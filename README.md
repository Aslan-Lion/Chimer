# Chimer

Chimer is a desktop timer and presentation application for Windows.

## Features

- **Dual-window interface:** Operate Chimer from its control window while displaying the timer separately.
- **External display support:** Present the timer on a second monitor, with positioning and fullscreen controls.
- **Timer controls:** Start, pause, resume, stop, and manage timer sessions.
- **Presets:** Save and reuse timer configurations.
- **Session builder:** Create structured sessions with multiple stages and transitions.
- **Display customization:** Customize layouts, typography, colors, and presentation appearance.
- **Remote control:** Pair supported devices over your local network.
- **Device permissions:** Manage paired devices and their access permissions.

## Platform

- Windows 10/11, 64-bit.
- Tauri 2, Rust, React, and TypeScript.

## Remote access and security

Remote control in version 0.1.0 is intended for trusted local networks.

The current local-network WebSocket connection does not use transport encryption. Do not expose it directly to the public internet or use it across untrusted networks.

Online relay networking and end-to-end encryption are future work and are not included in this release.

## Installation

Download the Windows installer from the official Chimer download page when a release is available.

Run the installer and follow the installation instructions.

## Data storage

Chimer stores application data in the Windows application data directory associated with:

`com.nyameso-idan.app-timer`

## License

Chimer is proprietary software. All rights are reserved by Nyameso Idan.

See `LICENSE.txt` for the copyright notice. Use of distributed application builds will be subject to separate end-user license terms.

## Version

Current development release: **0.1.0**
