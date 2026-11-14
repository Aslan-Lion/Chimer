//! The host's register of paired devices and the permissions attached to them.
//!
//! This is the only place a role or a permission is decided. A connection never
//! carries its own authorisation: it presents a token, the registry resolves the
//! token to a record, and the record's role and permissions are what the command
//! layer enforces. A device that reconnects is therefore handed exactly what it
//! had — never more.

use std::collections::HashSet;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

use super::model::{
    DevicePermission, DeviceRole, MAX_PAIRED_DEVICES, PairedDevice,
};
use super::tokens::{constant_time_eq, hash_token};

/// Every device the operator has approved so far.
#[derive(Debug, Default)]
pub struct DeviceRegistry {
    devices: Vec<PairedDevice>,
    revoked_keys: HashSet<String>,
}

impl DeviceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuilds the registry from a persisted document.
    ///
    /// Entries that are unusable — no id, no token digest — are dropped rather
    /// than repaired, so a corrupted record can never become a way in.
    pub fn restore(devices: Vec<PairedDevice>) -> Self {
        let devices = devices
            .into_iter()
            .filter(|device| !device.id.is_empty() && !device.token_hash.is_empty())
            .map(|mut device| {
                device.permissions = sanitise(device.role, device.permissions.clone());
                device
            })
            .collect();
        Self {
            devices,
            revoked_keys: HashSet::new(),
        }
    }

    /// The record as it is persisted.
    pub fn snapshot(&self) -> Vec<PairedDevice> {
        self.devices.clone()
    }

    pub fn devices(&self) -> &[PairedDevice] {
        &self.devices
    }

    pub fn len(&self) -> usize {
        self.devices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&PairedDevice> {
        self.devices.iter().find(|device| device.id == id)
    }

    /// Resolves a device token to its record, or `None` if it was never issued
    /// or has been revoked.
    ///
    /// The presented token is hashed and compared digest-to-digest in constant
    /// time, so neither the token nor a partial match is observable through
    /// timing.
    pub fn resolve_token(&self, token: &str) -> Option<&PairedDevice> {
        if token.is_empty() {
            return None;
        }
        let digest = hash_token(token);
        self.devices
            .iter()
            .find(|device| constant_time_eq(&device.token_hash, &digest))
    }

    /// Registers a newly approved device.
    pub fn add(&mut self, mut device: PairedDevice) -> Result<(), String> {
        if self.get(&device.id).is_some() {
            return Err("That device is already paired.".to_string());
        }
        if self.devices.len() >= MAX_PAIRED_DEVICES {
            return Err(format!(
                "At most {MAX_PAIRED_DEVICES} devices can be paired. Revoke one first."
            ));
        }
        device.permissions = sanitise(device.role, device.permissions.clone());
        self.devices.push(device);
        Ok(())
    }

    /// Changes a device's role, resetting its permissions to the role defaults.
    ///
    /// Changing a role is a deliberate narrowing or widening by the operator, so
    /// the previous permission set is not carried across: a device moved to
    /// *display* immediately loses every command it used to be able to send.
    pub fn set_role(&mut self, id: &str, role: DeviceRole, now: i64) -> Result<PairedDevice, String> {
        let device = self
            .devices
            .iter_mut()
            .find(|device| device.id == id)
            .ok_or_else(|| "That device is not paired.".to_string())?;
        device.role = role;
        device.permissions = role.default_permissions();
        device.last_seen_at = now;
        Ok(device.clone())
    }

    /// Replaces a device's permission set.
    pub fn set_permissions(
        &mut self,
        id: &str,
        permissions: Vec<DevicePermission>,
        now: i64,
    ) -> Result<PairedDevice, String> {
        let device = self
            .devices
            .iter_mut()
            .find(|device| device.id == id)
            .ok_or_else(|| "That device is not paired.".to_string())?;
        device.permissions = sanitise(device.role, permissions);
        device.last_seen_at = now;
        Ok(device.clone())
    }

    /// Removes a device. Its token immediately stops resolving, so a revoked
    /// device cannot come back with the credential it already holds.
    pub fn revoke(&mut self, id: &str) -> bool {
        if let Some(pos) = self.devices.iter().position(|device| device.id == id) {
            let removed = self.devices.remove(pos);
            if let Some(pk) = removed.device_static_pk {
                self.revoked_keys.insert(pk);
            }
            true
        } else {
            false
        }
    }

    /// Whether a static public key has been revoked.
    pub fn is_revoked_key(&self, pk: &[u8; 32]) -> bool {
        let b64 = URL_SAFE_NO_PAD.encode(pk);
        self.revoked_keys.contains(&b64)
    }

    /// Explicitly records a revoked key.
    pub fn revoke_key(&mut self, pk_b64: impl Into<String>) {
        self.revoked_keys.insert(pk_b64.into());
    }

    /// Finds a device by its static public key.
    pub fn get_by_static_pk(&self, pk: &[u8; 32]) -> Option<&PairedDevice> {
        let b64 = URL_SAFE_NO_PAD.encode(pk);
        self.devices.iter().find(|d| d.device_static_pk.as_deref() == Some(&b64))
    }

    /// Records that a device was seen.
    pub fn touch(&mut self, id: &str, now: i64) {
        if let Some(device) = self.devices.iter_mut().find(|device| device.id == id) {
            device.last_seen_at = now;
        }
    }

    pub fn clear(&mut self) {
        self.devices.clear();
    }
}

/// Normalises a permission set: de-duplicated, restricted to known permissions,
/// and emptied entirely for a read-only display.
fn sanitise(role: DeviceRole, permissions: Vec<DevicePermission>) -> Vec<DevicePermission> {
    if role == DeviceRole::Display {
        return Vec::new();
    }
    let mut out: Vec<DevicePermission> = Vec::new();
    for permission in DevicePermission::all() {
        if permissions.contains(permission) {
            out.push(*permission);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::model::DeviceKind;

    fn device(id: &str, token: &str, role: DeviceRole) -> PairedDevice {
        PairedDevice {
            id: id.to_string(),
            name: "Phone".to_string(),
            kind: DeviceKind::Phone,
            role,
            permissions: role.default_permissions(),
            token_hash: hash_token(token),
            paired_at: 0,
            last_seen_at: 0,
            user_agent: "test".to_string(),
            device_static_pk: None,
        }
    }

    #[test]
    fn a_token_resolves_to_its_device() {
        let mut registry = DeviceRegistry::new();
        registry.add(device("device-1", "token-1", DeviceRole::Controller)).unwrap();
        registry.add(device("device-2", "token-2", DeviceRole::Controller)).unwrap();

        assert_eq!(registry.resolve_token("token-2").map(|d| d.id.as_str()), Some("device-2"));
        assert!(registry.resolve_token("token-3").is_none());
        assert!(registry.resolve_token("").is_none());
    }

    #[test]
    fn revoking_stops_a_held_token_from_working() {
        let mut registry = DeviceRegistry::new();
        registry.add(device("device-1", "token-1", DeviceRole::Controller)).unwrap();
        assert!(registry.resolve_token("token-1").is_some());

        assert!(registry.revoke("device-1"));
        assert!(registry.resolve_token("token-1").is_none());
        assert!(!registry.revoke("device-1"), "revoking twice changes nothing");
    }

    #[test]
    fn a_display_never_holds_a_permission() {
        let mut registry = DeviceRegistry::new();
        let mut entry = device("device-1", "token-1", DeviceRole::Display);
        entry.permissions = DevicePermission::all().to_vec();
        registry.add(entry).unwrap();

        let stored = registry.get("device-1").unwrap();
        assert!(stored.permissions.is_empty());
        assert!(!stored.has(DevicePermission::TimerStart));
    }

    #[test]
    fn a_controller_can_send_what_it_was_granted() {
        let mut registry = DeviceRegistry::new();
        registry.add(device("device-1", "token-1", DeviceRole::Controller)).unwrap();
        assert!(registry.get("device-1").unwrap().has(DevicePermission::TimerStart));
    }

    #[test]
    fn narrowing_permissions_is_respected() {
        let mut registry = DeviceRegistry::new();
        registry.add(device("device-1", "token-1", DeviceRole::Controller)).unwrap();

        registry
            .set_permissions("device-1", vec![DevicePermission::TimerPause], 10)
            .unwrap();

        let stored = registry.get("device-1").unwrap();
        assert!(stored.has(DevicePermission::TimerPause));
        assert!(!stored.has(DevicePermission::TimerStart));
        assert_eq!(stored.last_seen_at, 10);
    }

    #[test]
    fn a_reconnect_cannot_widen_what_a_device_holds() {
        let mut registry = DeviceRegistry::new();
        registry.add(device("device-1", "token-1", DeviceRole::Controller)).unwrap();
        registry
            .set_permissions("device-1", vec![DevicePermission::TimerPause], 10)
            .unwrap();

        // The device comes back holding the same token; nothing about the
        // presentation of that token may restore the withdrawn permission.
        let resolved = registry.resolve_token("token-1").expect("the token is still valid");
        assert_eq!(resolved.permissions, vec![DevicePermission::TimerPause]);
        assert!(!resolved.has(DevicePermission::TimerStart));
    }

    #[test]
    fn moving_to_display_withdraws_every_permission() {
        let mut registry = DeviceRegistry::new();
        registry.add(device("device-1", "token-1", DeviceRole::Controller)).unwrap();

        let updated = registry.set_role("device-1", DeviceRole::Display, 5).unwrap();
        assert_eq!(updated.role, DeviceRole::Display);
        assert!(updated.permissions.is_empty());
        assert!(!updated.has(DevicePermission::TimerAbort));
    }

    #[test]
    fn unknown_permissions_are_discarded() {
        let mut registry = DeviceRegistry::new();
        registry.add(device("device-1", "token-1", DeviceRole::Controller)).unwrap();
        let updated = registry
            .set_permissions(
                "device-1",
                vec![DevicePermission::TimerStart, DevicePermission::TimerStart],
                0,
            )
            .unwrap();
        assert_eq!(updated.permissions, vec![DevicePermission::TimerStart]);
    }

    #[test]
    fn restoring_drops_unusable_records() {
        let mut broken = device("", "", DeviceRole::Controller);
        broken.id = String::new();
        let registry = DeviceRegistry::restore(vec![
            device("device-1", "token-1", DeviceRole::Controller),
            broken,
        ]);
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.devices()[0].id, "device-1");
    }

    #[test]
    fn the_pairing_capacity_is_enforced() {
        let mut registry = DeviceRegistry::new();
        for index in 0..MAX_PAIRED_DEVICES {
            registry
                .add(device(&format!("device-{index}"), &format!("token-{index}"), DeviceRole::Controller))
                .expect("the register is not full yet");
        }
        let overflow = registry.add(device("one-too-many", "token-x", DeviceRole::Controller));
        assert!(overflow.is_err());
    }

    #[test]
    fn operations_on_an_unknown_device_are_refused() {
        let mut registry = DeviceRegistry::new();
        assert!(registry.set_role("ghost", DeviceRole::Display, 0).is_err());
        assert!(registry.set_permissions("ghost", Vec::new(), 0).is_err());
    }
}
