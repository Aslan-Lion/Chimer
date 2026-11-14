# Timer adjustment controls

The desktop Control Window exposes `-x` and `+x` while the authoritative timer
is running, paused or completed. The selected interval is a persistent General setting;
the default is one minute. Rust validates each signed millisecond adjustment,
updates the timer state and deadline, persists it, and broadcasts the same
snapshot to both desktop windows.

Adjustment is intentionally not exposed to paired remote controllers in this
restoration. The remote command vocabulary and per-device permissions remain
unchanged, so existing controller grants gain no new capability and read-only
display devices remain read-only.

Idle and stopped timers are not adjustable: starting either uses its configured
full duration. Increasing a completed timer clears overtime and moves it to
paused with the added time remaining, so it cannot restart without an explicit
user action. Decreasing a completed timer reduces overtime first; if the
adjustment exceeds overtime, the timer moves to paused with the excess as
remaining time. An adjustment equal to overtime leaves the timer completed with
zero overtime. Reducing a running or paused timer to zero still completes it
through the normal completion state; increasing an expired running timer cannot
revive it because the engine settles the deadline before handling the command.
