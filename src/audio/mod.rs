pub mod capture;
pub mod protocol;
#[cfg(feature = "audio-client")]
pub mod receiver;
pub mod sync;
pub mod transport;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioOutput {
    Auto,
    Local,
    Remote,
    Disabled,
}

impl std::str::FromStr for AudioOutput {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "local" => Ok(Self::Local),
            "remote" => Ok(Self::Remote),
            "disabled" => Ok(Self::Disabled),
            _ => anyhow::bail!("--audio-output must be auto, local, remote or disabled"),
        }
    }
}
