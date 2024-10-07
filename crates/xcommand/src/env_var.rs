use eyre::Result;
use std::ffi::CString;

#[derive(Debug)]
pub struct EnvVar {
    pub key: CString,
    pub value: CString,
}
impl EnvVar {
    pub fn from_str_pair(key: &str, value: &str) -> Result<Self> {
        Ok(EnvVar {
            key: CString::new(key)?,
            value: CString::new(value)?,
        })
    }
}
