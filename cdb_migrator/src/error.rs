//! Errors returned when a CDB 1.x datastore cannot be read or a requested
//! migration is deliberately refused.
use std::fmt;
use std::path::PathBuf;

/// Failure to inspect or accept a CDB 1.x datastore.
#[derive(Debug)]
pub enum Cdb1Error {
    /// An underlying I/O failure, with the path that failed.
    Io(PathBuf, std::io::Error),
    /// The offered root is not a directory.
    NotADirectory(PathBuf),
    /// A metadata XML file this crate must understand did not parse.
    Xml(PathBuf, String),
    /// A well-formed situation this version deliberately does not handle.
    Refused(String),
}

impl fmt::Display for Cdb1Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cdb1Error::Io(path, err) => write!(f, "{}: {err}", path.display()),
            Cdb1Error::NotADirectory(path) => {
                write!(f, "{} is not a directory", path.display())
            }
            Cdb1Error::Xml(path, why) => write!(f, "{}: {why}", path.display()),
            Cdb1Error::Refused(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Cdb1Error {}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::PathBuf;

    use super::Cdb1Error;

    #[test]
    fn req_error_io_display_includes_path_and_cause() {
        let error = Cdb1Error::Io(
            PathBuf::from("source/CDB.xml"),
            io::Error::new(io::ErrorKind::NotFound, "missing"),
        );

        assert_eq!(error.to_string(), "source/CDB.xml: missing");
    }

    #[test]
    fn req_error_not_a_directory_display_includes_path() {
        let error = Cdb1Error::NotADirectory(PathBuf::from("source"));

        assert_eq!(error.to_string(), "source is not a directory");
    }

    #[test]
    fn req_error_xml_display_includes_path_and_reason() {
        let error = Cdb1Error::Xml(PathBuf::from("source/Version.xml"), "bad root".into());

        assert_eq!(error.to_string(), "source/Version.xml: bad root");
    }

    #[test]
    fn req_error_refused_display_is_reason() {
        let error = Cdb1Error::Refused("multiple roots are unsupported".into());

        assert_eq!(error.to_string(), "multiple roots are unsupported");
    }
}
