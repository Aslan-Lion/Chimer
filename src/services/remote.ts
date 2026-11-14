import type { ClientMessage, ProtocolError, ServerMessage } from "../types";
import { PROTOCOL_VERSION } from "../types";
import type { RemotePhase } from "../stores/remoteStore";
import { applyRemote, remoteStore, resetRemote } from "../stores/remoteStore";

/**
 * Remote (browser) transport for Milestone 6 local networking.
 *
 * A phone or tablet on the same Wi-Fi opens the host's page, which is the very
 * same bundle the desktop windows run. The router sends it here instead of the
 * control interface. This module owns the one WebSocket that page keeps to the
 * host and mirrors every authoritative message into {@link remoteStore}.
 *
 * Three rules are enforced here, matching the Rust side:
 *
 * * the remote never computes state — it copies what the host sends;
 * * a message whose revision is older than the last one applied is dropped, so a
 *   late frame can never roll the interface backwards after a reconnect;
 * * a command is only ever *asked for*; the host decides whether it runs.
 */

// ---------------------------------------------------------------------------
// Pairing code in the URL
// ---------------------------------------------------------------------------

/**
 * Reads the `#pair=` fragment the QR code encodes.
 *
 * The code lives in the fragment on purpose: a fragment is never sent to the
 * server, so the pairing code does not appear in any request log on the host.
 */
export function readPairCodeFromUrl(): string | null {
  if (typeof window === "undefined") return null;
  const hash = window.location.hash.replace(/^#/, "");
  if (!hash) return null;
  const params = new URLSearchParams(hash);
  const code = params.get("pair");
  return code && /^\d{4,8}$/.test(code) ? code : null;
}

/** Removes the `#pair=` fragment once it has been consumed. */
export function clearPairCodeFromUrl(): void {
  if (typeof window === "undefined") return;
  if (!window.location.hash) return;
  const url = `${window.location.pathname}${window.location.search}`;
  window.history.replaceState(null, "", url);
}

// ---------------------------------------------------------------------------
// Stored device credentials
// ---------------------------------------------------------------------------

interface Credentials {
  deviceId: string;
  token: string;
}

/**
 * The device token is the only credential a remote holds, and losing it means
 * re-pairing. It is kept in `localStorage`, which is scoped to the host's
 * origin, so one host's token is never visible to another.
 *
 * Limitation (see the security notes): `localStorage` is readable by any script
 * running on this origin and is not encrypted at rest. On a shared machine an
 * attacker with local access could lift the token; there is no server-side
 * remedy for that beyond the host's ability to revoke the device.
 */
const CREDENTIALS_KEY = "app-timer.remote.credentials";

function readCredentials(): Credentials | null {
  try {
    const raw = window.localStorage.getItem(CREDENTIALS_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<Credentials>;
    if (typeof parsed.deviceId === "string" && typeof parsed.token === "string") {
      return { deviceId: parsed.deviceId, token: parsed.token };
    }
  } catch {
    // A corrupt entry is treated as no entry; the user simply re-pairs.
  }
  return null;
}

function writeCredentials(credentials: Credentials): void {
  try {
    window.localStorage.setItem(CREDENTIALS_KEY, JSON.stringify(credentials));
  } catch {
    // Private-mode storage failures are non-fatal: the session still works.
  }
}

function dropCredentials(): void {
  try {
    window.localStorage.removeItem(CREDENTIALS_KEY);
  } catch {
    // Nothing to do if storage is unavailable.
  }
}

// ---------------------------------------------------------------------------
// Command plumbing
// ---------------------------------------------------------------------------

export interface CommandOutcome {
  ok: boolean;
  error: ProtocolError | null;
}

const COMMAND_TIMEOUT_MS = 8_000;
const HEARTBEAT_MS = 20_000;
const STALE_AFTER_MS = 50_000;
/** How long a socket may stay un-opened before it is treated as unreachable. */
const CONNECT_TIMEOUT_MS = 10_000;
/** How long the host may stay silent about a sent pairing request. */
const PAIR_TIMEOUT_MS = 20_000;
/** How long an approved device may wait for its first snapshot. */
const STATE_TIMEOUT_MS = 8_000;

function newCommandId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `cmd-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

// ---------------------------------------------------------------------------
// The client
// ---------------------------------------------------------------------------

interface PendingCommand {
  resolve: (outcome: CommandOutcome) => void;
  timer: number;
}

/**
 * Owns the single WebSocket a remote page keeps to the host.
 *
 * A module-level singleton is used because a remote page has exactly one
 * connection: creating a second client would open a second socket and confuse
 * the host's per-connection rate budget.
 */
class RemoteClient {
  private socket: WebSocket | null = null;
  private reconnectTimer: number | null = null;
  private heartbeatTimer: number | null = null;
  /** Deadline on a socket that has not reached `open` yet. */
  private connectTimer: number | null = null;
  /** Deadline on a pairing request the host has not acknowledged yet. */
  private pairTimer: number | null = null;
  private attempt = 0;
  private stopped = false;
  /** A pairing code supplied by the URL or typed by the user, not yet redeemed. */
  private pendingCode: string | null = null;
  private deviceName = "";
  private lastMessageAt = 0;
  private presentationRevision = 0;
  private readonly commands = new Map<string, PendingCommand>();

  /** Opens the connection, or leaves it running if it already is. */
  start(deviceName: string): void {
    this.deviceName = deviceName;
    this.stopped = false;
    if (this.pendingCode === null) this.pendingCode = readPairCodeFromUrl();
    applyRemote({ hasCredentials: readCredentials() !== null });
    if (this.isOpen() || this.isConnecting()) {
      return;
    }
    this.open();
  }

  /**
   * Redeems a pairing code, either from the URL or typed by the operator.
   *
   * A submit never depends on the socket already being open: the code is held
   * and transmitted the moment the host has greeted us, so a slow handshake
   * cannot swallow what the operator typed.
   */
  pair(code: string, deviceName: string): void {
    this.deviceName = deviceName;
    this.pendingCode = code.trim();
    applyRemote({ notice: null, commandError: null });
    if (this.isOpen()) {
      this.sendPair();
    } else {
      applyRemote({ phase: "connecting" });
      this.start(deviceName);
    }
  }

  /** Asks the host for the complete state again (used after a reconnect). */
  requestSnapshot(): void {
    this.send({ type: "snapshot" });
  }

  /** Sends one permitted action and resolves when the host answers. */
  command(action: string, payload?: unknown): Promise<CommandOutcome> {
    return new Promise((resolve) => {
      if (!this.socket || this.socket.readyState !== WebSocket.OPEN) {
        resolve({
          ok: false,
          error: { code: "offline", message: "Not connected to the host." },
        });
        return;
      }
      const id = newCommandId();
      const timer = window.setTimeout(() => {
        this.commands.delete(id);
        applyRemote({ pendingAction: null });
        resolve({
          ok: false,
          error: { code: "timeout", message: "The host did not answer in time." },
        });
      }, COMMAND_TIMEOUT_MS);

      this.commands.set(id, { resolve, timer });
      applyRemote({ pendingAction: action, commandError: null });
      this.send({ type: "command", id, action, payload });
    });
  }

  /**
   * Forgets the device: drops the token, discards the socket and returns to the
   * pairing screen.
   *
   * The socket is closed on purpose. The host marked *that* connection as
   * authenticated, so leaving it open would let a "pair again" request ride on
   * the very identity the operator is trying to replace.
   */
  forgetDevice(): void {
    dropCredentials();
    this.pendingCode = null;
    applyRemote({
      hasCredentials: false,
      phase: "pairing",
      notice: null,
      commandError: null,
      device: null,
      role: null,
      permissions: [],
    });
    this.restart();
  }

  /** Closes the socket and cancels any pending reconnect; used on unmount. */
  stop(): void {
    this.stopped = true;
    this.clearTimers();
    this.rejectAllCommands({ code: "closed", message: "The connection was closed." });
    this.discardSocket();
  }

  /**
   * Retries after a failure the operator has been shown.
   *
   * A held pairing code is re-sent rather than dropped: the host never accepted
   * it, so the operator should not have to type six digits again.
   */
  retry(): void {
    this.stopped = false;
    applyRemote({ notice: null, commandError: null });
    if (this.isOpen()) {
      // The socket survived; only the attempt failed.
      if (this.pendingCode) this.sendPair();
      else applyRemote({ phase: "pairing" });
      return;
    }
    this.restart();
  }

  // -------------------------------------------------------------- internals

  /** A phase that ends the exchange until the operator starts it again. */
  private isTerminalPhase(phase: RemotePhase): boolean {
    return phase === "revoked";
  }

  private isOpen(): boolean {
    return this.socket !== null && this.socket.readyState === WebSocket.OPEN;
  }

  private isConnecting(): boolean {
    return this.socket !== null && this.socket.readyState === WebSocket.CONNECTING;
  }

  /** Detaches and closes the current socket, leaving no handlers behind. */
  private discardSocket(): void {
    const socket = this.socket;
    this.socket = null;
    if (!socket) return;
    socket.onopen = null;
    socket.onmessage = null;
    socket.onerror = null;
    socket.onclose = null;
    try {
      socket.close();
    } catch {
      // Already closing.
    }
  }

  /** Drops the current socket and starts a fresh handshake on it. */
  private restart(): void {
    this.clearTimers();
    this.discardSocket();
    if (!this.stopped) this.open();
  }

  private clearTimers(): void {
    if (this.reconnectTimer !== null) {
      window.clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.heartbeatTimer !== null) {
      window.clearInterval(this.heartbeatTimer);
      this.heartbeatTimer = null;
    }
    if (this.connectTimer !== null) {
      window.clearTimeout(this.connectTimer);
      this.connectTimer = null;
    }
    if (this.pairTimer !== null) {
      window.clearTimeout(this.pairTimer);
      this.pairTimer = null;
    }
  }

  private clearConnectTimer(): void {
    if (this.connectTimer === null) return;
    window.clearTimeout(this.connectTimer);
    this.connectTimer = null;
  }

  private clearPairTimer(): void {
    if (this.pairTimer === null) return;
    window.clearTimeout(this.pairTimer);
    this.pairTimer = null;
  }

  /**
   * Gives up on an attempt that failed.
   *
   * A paired device is retried quietly in the background: it holds a token and
   * has nothing to ask the operator for. An unpaired page stops with an
   * explanation and a retry, because a pairing code needs a human either way and
   * a spinner with no ending tells them nothing.
   */
  private failAttempt(code: string, message: string): void {
    this.clearTimers();
    const current = remoteStore.get();
    if (this.isTerminalPhase(current.phase)) {
      applyRemote({ connected: false });
      return;
    }
    if (current.hasCredentials && !this.stopped) {
      this.scheduleReconnect();
      return;
    }
    applyRemote({
      connected: false,
      reconnectAttempt: 0,
      phase: "failed",
      notice: { code, message },
    });
  }

  /**
   * The deadline for the first snapshot after a device is approved.
   *
   * The token is already stored by then, so the device really is paired; asking
   * again beats leaving the interface on "Approved — signing in…" forever.
   */
  private armStateDeadline(): void {
    this.clearPairTimer();
    this.pairTimer = window.setTimeout(() => {
      this.pairTimer = null;
      if (remoteStore.get().phase !== "authenticating") return;
      applyRemote({
        phase: "online",
        notice: {
          code: "state-pending",
          message: "Paired. Waiting for the host to send the current timer state.",
        },
      });
      this.send({ type: "snapshot" });
    }, STATE_TIMEOUT_MS);
  }

  /**
   * Transmits the held pairing code and arms the acknowledgement deadline.
   *
   * The deadline is what stops a host that accepts the socket and then says
   * nothing from leaving the browser on a spinner forever.
   */
  private sendPair(): void {
    const code = this.pendingCode;
    if (!code) return;
    this.send({ type: "pair", code, name: this.deviceName || undefined });
    clearPairCodeFromUrl();

    this.clearPairTimer();
    this.pairTimer = window.setTimeout(() => {
      this.pairTimer = null;
      // The code is deliberately kept: the host never accepted it, so retrying
      // must not make the operator type six digits again.
      this.failAttempt(
        "pairing-timeout",
        "The host did not answer the pairing request. Check that local networking is on, then try again.",
      );
    }, PAIR_TIMEOUT_MS);
  }

  private rejectAllCommands(error: ProtocolError): void {
    for (const [, pending] of this.commands) {
      window.clearTimeout(pending.timer);
      pending.resolve({ ok: false, error });
    }
    this.commands.clear();
    if (remoteStore.get().pendingAction !== null) applyRemote({ pendingAction: null });
  }

  private socketUrl(): string {
    const scheme = window.location.protocol === "https:" ? "wss:" : "ws:";
    return `${scheme}//${window.location.host}/ws`;
  }

  private send(message: ClientMessage): void {
    if (!this.socket || this.socket.readyState !== WebSocket.OPEN) return;
    try {
      this.socket.send(JSON.stringify(message));
    } catch (error) {
      // A send failure is followed by the socket's own close event; it is
      // logged rather than dropped so the reason is never invisible.
      console.warn(`[app.timer remote] send failed: ${String(error)}`);
    }
  }

  private open(): void {
    this.clearTimers();
    applyRemote({ phase: "connecting", connected: false });

    const url = this.socketUrl();
    let socket: WebSocket;
    try {
      socket = new WebSocket(url);
    } catch {
      this.failAttempt(
        "unreachable",
        "Could not open a connection to the host. Check the address shown on the host, then try again.",
      );
      return;
    }
    this.socket = socket;

    // A socket that never reaches `open` (host stopped, wrong address, blocked
    // port) must be reported. Without this deadline the page would wait on
    // "Connecting…" for as long as the tab stays open.
    this.connectTimer = window.setTimeout(() => {
      this.connectTimer = null;
      this.discardSocket();
      this.failAttempt(
        "unreachable",
        "The host did not answer. Check that local networking is on and that this device is on the same network, then try again.",
      );
    }, CONNECT_TIMEOUT_MS);

    socket.onopen = () => {
      // TEMPORARY Milestone 6 diagnostics: a phone has no console visible next
      // to it, so the code and reason of every close are also put on screen.
      console.info(`[app.timer remote] socket open: ${url}`);
      this.clearConnectTimer();
      this.attempt = 0;
      this.lastMessageAt = Date.now();
      applyRemote({ connected: true, reconnectAttempt: 0, notice: null });
      this.send({ type: "hello", protocol: PROTOCOL_VERSION });
      this.heartbeatTimer = window.setInterval(() => this.heartbeat(), HEARTBEAT_MS);
    };

    socket.onmessage = (event: MessageEvent<string>) => {
      this.lastMessageAt = Date.now();
      this.receive(event.data);
    };

    socket.onerror = () => {
      // A browser never says *why* a socket failed; the close event that
      // follows carries the code. It is still recorded here so a failure is
      // never invisible in the console.
      console.warn(`[app.timer remote] socket error: ${url}`);
    };

    socket.onclose = (event: CloseEvent) => {
      console.warn(
        `[app.timer remote] socket closed: code=${event.code} reason=${
          event.reason || "<none>"
        } clean=${event.wasClean}`,
      );
      this.clearTimers();
      const wasSocket = this.socket;
      this.socket = null;
      if (wasSocket) {
        wasSocket.onopen = null;
        wasSocket.onmessage = null;
        wasSocket.onerror = null;
        wasSocket.onclose = null;
      }
      if (this.stopped) return;
      applyRemote({ connected: false });
      if (this.isTerminalPhase(remoteStore.get().phase)) return;
      // A paired device reconnects on its own; an unpaired page is told what
      // happened, because the pairing code needs the operator either way. The
      // close code is shown too: it is the only clue the page itself holds.
      this.failAttempt(
        "connection-lost",
        `The connection to the host dropped (code ${event.code}). Check the network, then try again.`,
      );
    };
  }

  /**
   * Retries the connection with a growing backoff.
   *
   * Only a device that already holds a token retries by itself: nobody has to
   * approve it, so a quiet retry is the right behaviour. An unpaired page stays
   * where it is and shows why instead of spinning.
   */
  private scheduleReconnect(): void {
    if (this.stopped || this.reconnectTimer !== null) return;
    if (!remoteStore.get().hasCredentials) return;
    const attempt = (this.attempt += 1);
    // 0.5s, 1s, 2s, 4s, 8s, then a steady 15s.
    const delay = Math.min(500 * 2 ** (attempt - 1), 15_000);
    applyRemote({ phase: "connecting", reconnectAttempt: attempt });
    this.reconnectTimer = window.setTimeout(() => {
      this.reconnectTimer = null;
      if (!this.stopped) this.open();
    }, delay);
  }

  private heartbeat(): void {
    if (!this.socket || this.socket.readyState !== WebSocket.OPEN) return;
    if (Date.now() - this.lastMessageAt > STALE_AFTER_MS) {
      // The socket looks open but the host has gone quiet; force a reconnect.
      try {
        this.socket.close();
      } catch {
        // The close handler will run regardless.
      }
      return;
    }
    this.send({ type: "ping", nonce: Date.now() });
  }

  // -------------------------------------------------------------- receiving

  private receive(raw: string): void {
    let message: ServerMessage;
    try {
      message = JSON.parse(raw) as ServerMessage;
    } catch (error) {
      console.warn(`[app.timer remote] ignored an unreadable message: ${String(error)}`);
      return;
    }

    switch (message.type) {
      case "welcome":
        applyRemote({ server: message.server });
        this.afterWelcome();
        break;

      case "pairing-pending":
        // The host has the request and the operator has to approve it. This is
        // the state that replaces the old endless "Connecting…".
        this.clearPairTimer();
        applyRemote({ phase: "awaiting-approval", notice: null, commandError: null });
        break;

      case "paired": {
        // Approved *and* authenticated: the token is the host's verdict. The
        // snapshot that follows is what makes the page `online`.
        this.clearPairTimer();
        writeCredentials({ deviceId: message.device.id, token: message.token });
        this.pendingCode = null;
        clearPairCodeFromUrl();
        applyRemote({
          device: message.device,
          role: message.device.role,
          permissions: message.device.permissions,
          phase: "authenticating",
          notice: null,
          commandError: null,
          hasCredentials: true,
        });
        this.armStateDeadline();
        break;
      }

      case "pairing-rejected": {
        this.clearPairTimer();
        this.pendingCode = null;
        // The host sends one message for both a refusal and an expired request,
        // so the reason is what tells them apart.
        const expired = /expir/i.test(message.reason);
        applyRemote({
          phase: expired ? "expired" : "failed",
          commandError: null,
          notice: expired
            ? {
                code: "pairing-expired",
                message:
                  "This pairing code expired before the host approved it. Ask the host for a new code, then pair again.",
              }
            : { code: "pairing-rejected", message: message.reason },
        });
        break;
      }

      case "device-updated":
        applyRemote({ role: message.role, permissions: message.permissions });
        break;

      case "revoked":
        dropCredentials();
        this.pendingCode = null;
        this.clearPairTimer();
        applyRemote({
          phase: "revoked",
          connected: false,
          hasCredentials: false,
          device: null,
          role: null,
          permissions: [],
          notice: { code: "revoked", message: message.reason },
        });
        break;

      case "snapshot": {
        const { snapshot } = message;
        this.clearPairTimer();
        this.presentationRevision = snapshot.revision;
        applyRemote({
          timer: snapshot.timer,
          session: snapshot.session,
          sessionPresentation: snapshot.sessionPresentation,
          presentation: snapshot.presentation,
          revision: snapshot.revision,
          libraryRevision: snapshot.libraryRevision,
          phase: "online",
        });
        break;
      }

      case "state": {
        if (message.revision < remoteStore.get().revision) return;
        applyRemote({
          timer: message.timer,
          session: message.session,
          sessionPresentation: message.sessionPresentation,
          revision: message.revision,
        });
        break;
      }

      case "presentation":
        if (message.revision < this.presentationRevision) return;
        this.presentationRevision = message.revision;
        applyRemote({ presentation: message.presentation });
        break;

      case "library":
        if (message.revision < remoteStore.get().libraryRevision) return;
        applyRemote({
          presets: message.presets,
          sessions: message.sessions,
          libraryRevision: message.revision,
        });
        break;

      case "command-result": {
        const pending = this.commands.get(message.id);
        if (pending) {
          window.clearTimeout(pending.timer);
          this.commands.delete(message.id);
          pending.resolve({ ok: message.ok, error: message.error });
        }
        applyRemote({
          pendingAction: null,
          commandError: message.ok ? null : message.error,
        });
        break;
      }

      case "error":
        this.handleError(message.code, message.message);
        break;

      case "pong":
        break;
    }
  }

  /** Decides the next step once the host has greeted us. */
  private afterWelcome(): void {
    if (remoteStore.get().phase === "online") return;

    if (this.pendingCode) {
      this.sendPair();
      return;
    }

    const credentials = readCredentials();
    if (credentials) {
      applyRemote({ phase: "authenticating" });
      this.send({ type: "authenticate", deviceId: credentials.deviceId, token: credentials.token });
      this.armStateDeadline();
      return;
    }

    applyRemote({ phase: "pairing" });
  }

  private handleError(code: string, message: string): void {
    if (code === "unauthenticated" || code === "revoked") {
      dropCredentials();
      this.clearPairTimer();
      applyRemote({
        phase: "pairing",
        hasCredentials: false,
        device: null,
        role: null,
        permissions: [],
        notice: { code, message },
      });
      return;
    }

    if (code === "unsupported-protocol") {
      this.clearPairTimer();
      applyRemote({ notice: { code, message }, phase: "failed" });
      return;
    }

    // A code the host did not accept ends the attempt. The host's own wording is
    // shown, because it is the only side that knows whether the code was wrong
    // or had already expired.
    if (code === "invalid-code" || code === "rate-limited") {
      this.clearPairTimer();
      this.pendingCode = null;
      applyRemote({ phase: "failed", notice: { code, message } });
      return;
    }

    // A rejected command already surfaces an error of its own.
    if (remoteStore.get().pendingAction !== null) return;
    applyRemote({ notice: { code, message } });
  }
}

export const remoteClient = new RemoteClient();

/** Starts (or resumes) the remote connection. Idempotent. */
export function connectRemote(deviceName: string): void {
  remoteClient.start(deviceName);
}

/** Closes the remote connection and forgets the transient state. */
export function disconnectRemote(): void {
  remoteClient.stop();
  resetRemote();
}

/** Redeems a pairing code typed by the operator. */
export function pairRemote(code: string, deviceName: string): void {
  remoteClient.pair(code, deviceName);
}

/** Sends one permitted action; resolves with the host's verdict. */
export function sendRemoteCommand(action: string, payload?: unknown): Promise<CommandOutcome> {
  return remoteClient.command(action, payload);
}

/** Forgets this browser's device token so the pairing screen returns. */
export function forgetRemoteDevice(): void {
  remoteClient.forgetDevice();
}

/** Retries after a failure, keeping a code the host never accepted. */
export function retryRemote(): void {
  remoteClient.retry();
}

/** Waits until the remote reports `online` or the timeout elapses. */
export function whenOnline(timeoutMs: number): Promise<boolean> {
  if (remoteStore.get().phase === "online") return Promise.resolve(true);
  return new Promise((resolve) => {
    const timer = window.setTimeout(() => {
      unsubscribe();
      resolve(false);
    }, timeoutMs);
    const unsubscribe = remoteStore.subscribe(() => {
      if (remoteStore.get().phase === "online") {
        window.clearTimeout(timer);
        unsubscribe();
        resolve(true);
      }
    });
  });
}
