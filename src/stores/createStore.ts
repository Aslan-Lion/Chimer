/**
 * Tiny external store built on `useSyncExternalStore`.
 *
 * The application deliberately avoids a state-management dependency: the
 * shapes needed here are small and a shared store keeps the two windows on an
 * identical contract once the Rust engine starts broadcasting updates.
 */
import { useSyncExternalStore } from "react";

export interface Store<T extends object> {
  get: () => T;
  set: (patch: Partial<T> | ((previous: T) => Partial<T>)) => void;
  subscribe: (listener: () => void) => () => void;
  use: () => T;
}

export function createStore<T extends object>(initialState: T): Store<T> {
  let state = initialState;
  const listeners = new Set<() => void>();

  const get = (): T => state;

  const set: Store<T>["set"] = (patch) => {
    const next = typeof patch === "function" ? patch(state) : patch;
    state = { ...state, ...next };
    listeners.forEach((listener) => listener());
  };

  const subscribe = (listener: () => void): (() => void) => {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  };

  const use = (): T => useSyncExternalStore(subscribe, get, get);

  return { get, set, subscribe, use };
}
