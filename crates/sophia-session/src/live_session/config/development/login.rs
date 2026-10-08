//! Read the caller's session from the server, not libsystemd's cgroup parser.
use std::collections::HashMap;
use std::time::Duration;
use zbus::blocking::{Connection, Proxy, connection::Builder};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Structure};

use super::Observation;

const NAME: &str = "org.freedesktop.login1";
const SESSION: &str = "org.freedesktop.login1.Session";
const MANAGER: &str = "org.freedesktop.login1.Manager";
const ROOT: &str = "/org/freedesktop/login1";

pub(super) struct Login(Connection);

impl Login {
    pub(super) fn connect() -> Result<Self, String> {
        // Never consult DBUS_SYSTEM_BUS_ADDRESS: an untrusted bus may answer
        // with made-up login facts. The host bus authenticates the caller.
        Builder::address("unix:path=/run/dbus/system_bus_socket")
            .and_then(|builder| builder.method_timeout(Duration::from_secs(2)).build())
            .map(Self)
            .map_err(|error| format!("development login system bus: {error}"))
    }

    fn owner(&self) -> zbus::Result<String> {
        let bus = Proxy::new(
            &self.0,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )?;
        let name: String = bus.call("GetNameOwner", &(NAME,))?;
        let uid: u32 = bus.call("GetConnectionUnixUser", &(&name,))?;
        if uid != 0 {
            return Err(zbus::Error::Failure(
                "login service is not root-owned".into(),
            ));
        }
        Ok(name)
    }

    pub(super) fn observe(&self) -> Result<Observation, String> {
        self.read()
            .map_err(|error| format!("development login observation: {error}"))
    }

    fn read(&self) -> zbus::Result<Observation> {
        let owner = self.owner()?;
        let manager = Proxy::new(&self.0, owner.as_str(), ROOT, MANAGER)?;
        // Zero means the authenticated bus caller. A PID from a nested PID
        // namespace is never substituted for the host's caller identity.
        let path: OwnedObjectPath = manager.call("GetSessionByPID", &(0u32,))?;
        let properties = Proxy::new(
            &self.0,
            owner.as_str(),
            path.as_str(),
            "org.freedesktop.DBus.Properties",
        )?;
        // GetAll each time; no proxy property cache can turn two observations
        // into two reads of the same cached value.
        let mut values: HashMap<String, OwnedValue> = properties.call("GetAll", &(SESSION,))?;
        let (seat, seat_path): (String, OwnedObjectPath) =
            Structure::try_from(take(&mut values, "Seat")?)?.try_into()?;
        let (uid, _): (u32, OwnedObjectPath) =
            Structure::try_from(take(&mut values, "User")?)?.try_into()?;
        let seat_properties = Proxy::new(
            &self.0,
            owner.as_str(),
            seat_path.as_str(),
            "org.freedesktop.DBus.Properties",
        )?;
        let seat_has_vts: OwnedValue =
            seat_properties.call("Get", &("org.freedesktop.login1.Seat", "CanTTY"))?;
        let result = Observation {
            owner: owner.clone(),
            path: path.to_string(),
            id: String::try_from(take(&mut values, "Id")?)?,
            uid,
            seat,
            seat_path: seat_path.to_string(),
            active: bool::try_from(take(&mut values, "Active")?)?,
            remote: bool::try_from(take(&mut values, "Remote")?)?,
            kind: String::try_from(take(&mut values, "Type")?)?,
            class: String::try_from(take(&mut values, "Class")?)?,
            tty: String::try_from(take(&mut values, "TTY")?)?,
            vt: u32::try_from(take(&mut values, "VTNr")?)?,
            seat_has_vts: bool::try_from(seat_has_vts)?,
        };
        if self.owner()? != owner {
            return Err(zbus::Error::Failure(
                "login service changed during observation".into(),
            ));
        }
        Ok(result)
    }
}

fn take(values: &mut HashMap<String, OwnedValue>, key: &str) -> zbus::Result<OwnedValue> {
    values
        .remove(key)
        .ok_or_else(|| zbus::Error::Failure(format!("missing login property {key}")))
}
