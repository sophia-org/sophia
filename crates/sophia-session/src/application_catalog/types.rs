use sophia_protocol::ShellApplicationDescriptor;
use std::path::PathBuf;

pub const MAX_DESKTOP_FILE_BYTES: usize = 65_536;
pub const MAX_CATALOG_SOURCE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_CATALOG_SCAN_ENTRIES: usize = 16_384;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationLaunchCommand {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub working_directory: Option<PathBuf>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredCatalogApplication {
    pub name: String,
    pub command: ApplicationLaunchCommand,
}
#[derive(Clone, Debug)]
pub struct ApplicationCatalogEnvironment {
    pub search_path: Vec<PathBuf>,
    pub locale: String,
    pub current_desktop: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct ApplicationCatalogEntry {
    /// Stable Session-owned name, independent of label, slot and locale.
    pub identity: String,
    pub descriptor: ShellApplicationDescriptor,
    pub command: Option<ApplicationLaunchCommand>,
    pub(super) source: Option<(PathBuf, Vec<u8>)>,
}
#[derive(Clone, Debug, Default)]
pub struct ApplicationCatalog {
    pub entries: Vec<ApplicationCatalogEntry>,
    pub skipped: usize,
}
