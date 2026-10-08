//! Secondary native sessions must belong to their own active, non-VT login.
//! Output selection and ambient XDG variables alone grant no seat authority.
use std::time::Duration;

#[path = "development/login.rs"]
mod login;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DevelopmentSeat(String);

pub(super) fn from_arguments(
    args: &[String],
    native: bool,
    no_input: bool,
    physical_proof: bool,
    runtime: Option<Duration>,
) -> Result<Option<DevelopmentSeat>, String> {
    let seat = crate::support::arg_value(args, "--development-seat")
        .map(DevelopmentSeat::parse)
        .transpose()?;
    if let Some(seat) = &seat {
        seat.validate_arguments(native, no_input, physical_proof, runtime)?;
    }
    Ok(seat)
}

impl DevelopmentSeat {
    pub(super) fn parse(seat: String) -> Result<Self, String> {
        if seat == "seat0"
            || !seat.starts_with("seat")
            || !(5..=63).contains(&seat.len())
            || !seat
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err("--development-seat requires a named non-seat0 seat".into());
        }
        Ok(Self(seat))
    }

    pub(super) fn validate_arguments(
        &self,
        native: bool,
        no_input: bool,
        physical_proof: bool,
        runtime: Option<Duration>,
    ) -> Result<(), String> {
        if !native || !no_input || physical_proof {
            return Err("--development-seat requires --native-scanout --no-input without a physical input proof".into());
        }
        if !runtime.is_some_and(|bound| !bound.is_zero() && bound <= Duration::from_secs(300)) {
            return Err("--development-seat requires --max-runtime-ms in 1..=300000".into());
        }
        Ok(())
    }

    pub(super) fn attest(&self) -> Result<Admission, String> {
        let environment = Environment::current()?;
        environment.validate_backend()?;
        let uid = rustix::process::getuid().as_raw();
        if uid == 0 || uid != rustix::process::geteuid().as_raw() {
            return Err("development seat requires an unprivileged login".into());
        }
        let bus = login::Login::connect()?;
        admit(self, uid, &environment, || bus.observe())
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct Admission(Observation);

impl Admission {
    pub(super) fn check_opened_seat(&self, opened: &str, subsequent: &Self) -> Result<(), String> {
        if opened != self.0.seat || self != subsequent {
            return Err("development login changed or libseat opened another seat".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Observation {
    owner: String,
    path: String,
    id: String,
    uid: u32,
    seat: String,
    seat_path: String,
    active: bool,
    remote: bool,
    kind: String,
    class: String,
    tty: String,
    vt: u32,
    seat_has_vts: bool,
}

#[derive(Default)]
struct Environment {
    backend: Option<String>,
    session: Option<String>,
    seat: Option<String>,
    vt: Option<String>,
    kind: Option<String>,
    bus: Option<String>,
}

impl Environment {
    fn current() -> Result<Self, String> {
        let read = |name| match std::env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => {
                Err(format!("invalid development environment: {name}"))
            }
        };
        Ok(Self {
            backend: read("LIBSEAT_BACKEND")?,
            session: read("XDG_SESSION_ID")?,
            seat: read("XDG_SEAT")?,
            vt: read("XDG_VTNR")?,
            kind: read("XDG_SESSION_TYPE")?,
            bus: read("DBUS_SYSTEM_BUS_ADDRESS")?,
        })
    }

    fn validate_backend(&self) -> Result<(), String> {
        if self.backend.as_deref() != Some("logind") || self.bus.is_some() {
            return Err("development seat requires LIBSEAT_BACKEND=logind and the host system bus without override".into());
        }
        Ok(())
    }
}

fn admit(
    seat: &DevelopmentSeat,
    uid: u32,
    environment: &Environment,
    mut observe: impl FnMut() -> Result<Observation, String>,
) -> Result<Admission, String> {
    environment.validate_backend()?;
    let first = observe()?;
    let second = observe()?;
    if first != second {
        return Err("development login changed between observations".into());
    }
    if first.seat != seat.0
        || first.uid != uid
        || !first.active
        || first.remote
        || first.class != "user"
        || !matches!(first.kind.as_str(), "wayland" | "x11")
        || first.seat_has_vts
        || first.vt != 0
        || !first.tty.is_empty()
    {
        return Err(
            "development login must be active, local, graphical and without a TTY on its own seat"
                .into(),
        );
    }
    // libseat's logind backend otherwise falls back to the UID's display
    // session. Require the explicit ID, and independently prove it belongs to
    // this caller before libseat is allowed to take control of that session.
    if environment.session.as_deref() != Some(first.id.as_str())
        || environment
            .seat
            .as_deref()
            .is_some_and(|value| value != first.seat)
        || environment
            .vt
            .as_deref()
            .is_some_and(|value| value != "0" && !value.is_empty())
        || environment
            .kind
            .as_deref()
            .is_some_and(|value| value != first.kind)
    {
        return Err("development login disagrees with inherited XDG session identity".into());
    }
    Ok(Admission(first))
}

#[path = "../../../tests/support/development_seat.rs"]
mod tests;
