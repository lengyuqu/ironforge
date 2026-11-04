pub mod cargo;
pub mod composer;
pub mod docker;
pub mod generic;
pub mod helm;
pub mod maven;
pub mod npm;
pub mod nuget;
pub mod pypi;
pub mod rubygems;

pub use cargo::CargoAdapter;
pub use composer::ComposerAdapter;
pub use docker::DockerAdapter;
pub use generic::GenericAdapter;
pub use helm::HelmAdapter;
pub use maven::MavenAdapter;
pub use npm::NpmAdapter;
pub use nuget::NuGetAdapter;
pub use pypi::PyPIAdapter;
pub use rubygems::RubyGemsAdapter;

/// Name of a ZIP entry, for matching only.
///
/// zip 9 reports a name it cannot decode as an error rather than guessing. For
/// the suffix checks the adapters do (`.nuspec`, `composer.json`, `METADATA`)
/// an undecodable name is simply compared as lossy text, so one odd entry
/// cannot make an otherwise valid package unreadable.
pub(crate) fn zip_entry_name<R: std::io::Read + ?Sized>(entry: &zip::read::ZipFile<'_, R>) -> String {
    match entry.name() {
        Ok(name) => name.into_owned(),
        Err(_) => String::from_utf8_lossy(entry.name_raw()).into_owned(),
    }
}
