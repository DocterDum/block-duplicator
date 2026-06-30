use std::fmt;

#[derive(Debug, Clone)]
pub struct RuntimeContext {
    pub os: &'static str,
    pub is_elevated: bool,
    pub raw_disk_access_requires_elevation: bool,
}

#[derive(Debug, Clone)]
pub enum EnvironmentError {
    ElevationRequired,
}

impl fmt::Display for EnvironmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnvironmentError::ElevationRequired => {
                write!(
                    f,
                    "Administrative privileges are required for raw block-device access."
                )
            }
        }
    }
}

impl std::error::Error for EnvironmentError {}

pub fn validate_privileges_and_environment() -> Result<RuntimeContext, EnvironmentError> {
    #[cfg(target_os = "windows")]
    {
        let elevated = is_elevated::is_elevated();
        let context = RuntimeContext {
            os: "windows",
            is_elevated: elevated,
            raw_disk_access_requires_elevation: true,
        };

        if !elevated {
            return Err(EnvironmentError::ElevationRequired);
        }

        return Ok(context);
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(RuntimeContext {
            os: std::env::consts::OS,
            is_elevated: true,
            raw_disk_access_requires_elevation: false,
        })
    }
}
