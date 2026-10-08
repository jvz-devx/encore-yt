//! The device list and session state shared with the frontend. Connection
//! addresses and the remote deck stay inside the backend.

pub use encore_cast::Kind;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub model: String,
    pub group: bool,
}

impl From<&encore_cast::Device> for Device {
    fn from(device: &encore_cast::Device) -> Self {
        Self {
            id: device.id().into(),
            kind: device.kind(),
            name: device.name().into(),
            model: device.model().into(),
            group: matches!(device, encore_cast::Device::Cast(d) if d.is_group()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Session {
    Connecting(Device),
    Active(Device),
    Confirm { device: Device, app: String },
    Disconnecting(Device),
}

impl Session {
    pub fn device(&self) -> &Device {
        match self {
            Self::Connecting(d)
            | Self::Active(d)
            | Self::Disconnecting(d)
            | Self::Confirm { device: d, .. } => d,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub devices: Vec<Device>,
    pub scanning: bool,
    pub session: Option<Session>,
    pub error: Option<String>,
}
