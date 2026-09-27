//! Supported-subset readers for Version.xml and Configuration.xml.
//!
//! The fields follow the examples and prose in OGC 15-113r6 §10.1.7 and
//! §10.1.11 (also §3.2). These are not XSD validators: the complete historical
//! schemas have not been verified. Missing declarations remain missing; raw
//! unknown declarations and documented extension provenance remain available.
//! Recognizing a declaration or extension does not certify its semantics.
//!
//! Only unqualified elements, XML 1.0/UTF-8, and the documented controls below
//! are supported. Unknown syntax, ambiguous structure, DTDs, and processing
//! instructions are refused. Callers can attach a source path using
//! [`crate::Cdb1Error::Xml`] and inventory the refusal without losing source bytes.

use std::collections::BTreeMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

/// Recognized declaration strings, not guarantees of historical conformance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredVersion {
    V1_0,
    V1_1,
    V1_2,
    V3_0,
    V3_1,
    V3_2,
}

impl DeclaredVersion {
    /// Recognize an exact declaration without trimming or guessing.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "1.0" => Some(Self::V1_0),
            "1.1" => Some(Self::V1_1),
            "1.2" => Some(Self::V1_2),
            "3.0" => Some(Self::V3_0),
            "3.1" => Some(Self::V3_1),
            "3.2" => Some(Self::V3_2),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::V1_0 => "1.0",
            Self::V1_1 => "1.1",
            Self::V1_2 => "1.2",
            Self::V3_0 => "3.0",
            Self::V3_1 => "3.1",
            Self::V3_2 => "3.2",
        }
    }

    /// Whether the declaration names a pre-OGC version.
    pub fn is_pre_ogc(&self) -> bool {
        matches!(self, Self::V3_0 | Self::V3_1 | Self::V3_2)
    }
}

/// Recorded extension provenance. Its presence does not mean it is implemented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionDeclaration {
    pub name: String,
    pub version: String,
}

/// Supported declarations from a Version element.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct VersionXml {
    pub specification: Option<DeclaredVersion>,
    /// Decoded, untrimmed declaration, retained even when empty or unknown.
    pub specification_raw: Option<String>,
    pub specification_authority: Option<String>,
    pub specification_update: Option<String>,
    /// A present but blank name is refused, never converted to absence.
    pub previous_root: Option<String>,
    /// Decoded text and CDATA, including whitespace and all text segments.
    pub comment: Option<String>,
    pub metadata_standard: Option<String>,
    /// Provenance only; downstream policy must decide whether it is supported.
    pub extension: Option<ExtensionDeclaration>,
}

impl VersionXml {
    /// Read the supported Version.xml subset; errors are not XSD verdicts.
    pub fn parse(xml: &str) -> Result<Self, String> {
        declaration(&document(xml, "Version")?)
    }
}

/// One Configuration/Version entry, with its own declaration context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigurationVersion {
    pub folder: String,
    pub declaration: VersionXml,
}

/// Configuration entries in source order; no version is selected by the parser.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ConfigurationXml {
    /// Convenience projection of `versions`, in the same order.
    pub version_folders: Vec<String>,
    pub versions: Vec<ConfigurationVersion>,
    pub comment: Option<String>,
}

impl ConfigurationXml {
    /// Read the supported Configuration.xml subset; errors are not XSD verdicts.
    pub fn parse(xml: &str) -> Result<Self, String> {
        let root = document(xml, "Configuration")?;
        let mut out = Self::default();
        for child in &root.children {
            if child.name == "Comment" {
                out.comment = Some(child.text.clone());
            } else {
                let folder = child
                    .children
                    .iter()
                    .find(|node| node.name == "Folder")
                    .ok_or_else(|| refusal("Configuration/Version requires Folder"))?;
                let folder = nonblank(folder.required("path")?, "Folder path")?;
                out.version_folders.push(folder.clone());
                out.versions.push(ConfigurationVersion {
                    folder,
                    declaration: declaration(child)?,
                });
            }
        }
        Ok(out)
    }
}

struct Node {
    name: String,
    attrs: BTreeMap<String, String>,
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn required(&self, name: &str) -> Result<String, String> {
        self.attrs
            .get(name)
            .cloned()
            .ok_or_else(|| refusal(&format!("{} requires attribute {name}", self.name)))
    }
}

fn refusal(reason: &str) -> String {
    format!("metadata XML supported-subset refusal: {reason}")
}

fn nonblank(value: String, what: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        Err(refusal(&format!("{what} is present but blank")))
    } else {
        Ok(value)
    }
}

fn declaration(node: &Node) -> Result<VersionXml, String> {
    let mut out = VersionXml::default();
    for child in &node.children {
        match child.name.as_str() {
            "Specification" => {
                let raw = child.required("version")?;
                out.specification = DeclaredVersion::parse(&raw);
                out.specification_raw = Some(raw);
                out.specification_authority = child.attrs.get("authority").cloned();
                out.specification_update = child.attrs.get("update").cloned();
            }
            "PreviousIncrementalRootDirectory" => {
                out.previous_root = Some(nonblank(child.required("name")?, "previous root")?);
            }
            "Comment" => out.comment = Some(child.text.clone()),
            "Metadata" => out.metadata_standard = Some(child.required("standard")?),
            "Extension" => {
                out.extension = Some(ExtensionDeclaration {
                    name: child.required("name")?,
                    version: child.required("version")?,
                })
            }
            "Folder" => {} // ConfigurationXml retains it separately.
            _ => return Err(refusal("unsupported declaration control")),
        }
    }
    Ok(out)
}

fn xml_chars(text: &str) -> Result<(), String> {
    if text.chars().all(|c| matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')) {
        Ok(())
    } else {
        Err(refusal("invalid XML 1.0 character"))
    }
}

fn attributes(
    e: &BytesStart<'_>,
    reader: &Reader<&[u8]>,
    allowed: &[&str],
) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    // quick-xml accepts adjacent quoted attributes; our subset requires XML whitespace.
    let raw = e.attributes_raw();
    let mut quote = None;
    for (index, byte) in raw.iter().copied().enumerate() {
        if quote == Some(byte) {
            quote = None;
            if raw
                .get(index + 1)
                .is_some_and(|next| !matches!(next, b' ' | b'\t' | b'\n' | b'\r'))
            {
                return Err(refusal("attributes require separating whitespace"));
            }
        } else if quote.is_none() && matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
        }
    }
    // The default duplicate checks are intentional, including non-primary attrs.
    for attr in e.attributes() {
        let attr = attr.map_err(|e| refusal(&e.to_string()))?;
        let key = std::str::from_utf8(attr.key.as_ref()).map_err(|e| refusal(&e.to_string()))?;
        if !allowed.contains(&key) || attr.value.contains(&b'<') {
            return Err(refusal(&format!(
                "unsupported attribute or malformed value: {key}"
            )));
        }
        let value = attr
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|e| refusal(&e.to_string()))?
            .into_owned();
        xml_chars(&value)?;
        out.insert(key.to_owned(), value);
    }
    Ok(out)
}

fn start_node(
    e: &BytesStart<'_>,
    reader: &Reader<&[u8]>,
    stack: &[Node],
    root_name: &str,
) -> Result<Node, String> {
    let name = e.name();
    let name = std::str::from_utf8(name.as_ref()).map_err(|e| refusal(&e.to_string()))?;
    let permitted = match stack.last() {
        None => name == root_name,
        Some(parent) if parent.name == "Configuration" => matches!(name, "Comment" | "Version"),
        Some(parent) if parent.name == "Version" => {
            matches!(name, "Comment" | "Specification" | "Metadata" | "Extension")
                || (root_name == "Version" && name == "PreviousIncrementalRootDirectory")
                || (root_name == "Configuration" && name == "Folder")
        }
        _ => false,
    };
    if !permitted {
        return Err(refusal(&format!(
            "wrong root, unsupported or misplaced element: {name}"
        )));
    }
    if let Some(parent) = stack.last() {
        if name != "Version" && parent.children.iter().any(|child| child.name == name) {
            return Err(refusal(&format!("duplicate singleton element: {name}")));
        }
    }
    let allowed: &[&str] = match name {
        "Specification" => &["version", "authority", "update"],
        "Metadata" => &["standard"],
        "Extension" => &["name", "version"],
        "PreviousIncrementalRootDirectory" => &["name"],
        "Folder" => &["path"],
        _ => &[],
    };
    Ok(Node {
        name: name.to_owned(),
        attrs: attributes(e, reader, allowed)?,
        text: String::new(),
        children: Vec::new(),
    })
}

fn finish_node(stack: &mut Vec<Node>, root: &mut Option<Node>) -> Result<(), String> {
    let node = stack
        .pop()
        .ok_or_else(|| refusal("unexpected closing tag"))?;
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else if root.replace(node).is_some() {
        return Err(refusal("multiple root elements"));
    }
    Ok(())
}

fn text_content(stack: &mut [Node], text: &str, plain: bool) -> Result<(), String> {
    xml_chars(text)?;
    if let Some(node) = stack.last_mut() {
        if node.name == "Comment" {
            node.text.push_str(text);
            return Ok(());
        }
    }
    if plain && text.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r')) {
        Ok(())
    } else {
        Err(refusal("text, CDATA or reference outside Comment"))
    }
}

fn document(xml: &str, root_name: &str) -> Result<Node, String> {
    xml_chars(xml)?;
    let mut reader = Reader::from_str(xml.strip_prefix('\u{feff}').unwrap_or(xml));
    reader.config_mut().check_comments = true;
    let mut stack = Vec::new();
    let mut root = None;
    loop {
        let position = reader.buffer_position();
        let event = reader.read_event().map_err(|e| refusal(&e.to_string()))?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                if root.is_some() && stack.is_empty() {
                    return Err(refusal("multiple root elements"));
                }
                let node = start_node(&e, &reader, &stack, root_name)?;
                stack.push(node);
                if empty {
                    finish_node(&mut stack, &mut root)?;
                }
            }
            Event::End(_) => finish_node(&mut stack, &mut root)?,
            Event::Text(e) => {
                if e.as_ref().windows(3).any(|w| w == b"]]>") {
                    return Err(refusal("CDATA terminator in ordinary text"));
                }
                text_content(
                    &mut stack,
                    &e.xml10_content().map_err(|e| refusal(&e.to_string()))?,
                    true,
                )?;
            }
            Event::CData(e) => text_content(
                &mut stack,
                &e.xml10_content().map_err(|e| refusal(&e.to_string()))?,
                false,
            )?,
            Event::GeneralRef(e) => {
                let reference = e.decode().map_err(|e| refusal(&e.to_string()))?;
                let escaped = format!("&{reference};");
                let text =
                    quick_xml::escape::unescape(&escaped).map_err(|e| refusal(&e.to_string()))?;
                text_content(&mut stack, &text, false)?;
            }
            Event::Decl(e) => {
                if position != 0
                    || e.version().map_err(|e| refusal(&e.to_string()))?.as_ref() != b"1.0"
                {
                    return Err(refusal("XML declaration must be initial and version 1.0"));
                }
                let raw = std::str::from_utf8(e.as_ref()).map_err(|e| refusal(&e.to_string()))?;
                if raw.contains('&') {
                    return Err(refusal("references in XML declaration"));
                }
                let declaration = BytesStart::from_content(raw, 3);
                let attrs = attributes(
                    &declaration,
                    &reader,
                    &["version", "encoding", "standalone"],
                )?;
                let mut last_rank = 0;
                for attr in declaration.attributes() {
                    let attr = attr.map_err(|e| refusal(&e.to_string()))?;
                    let rank = match attr.key.as_ref() {
                        b"version" => 0,
                        b"encoding" => 1,
                        b"standalone" => 2,
                        _ => return Err(refusal("unsupported XML declaration attribute")),
                    };
                    if rank < last_rank {
                        return Err(refusal("XML declaration attributes are out of order"));
                    }
                    last_rank = rank;
                }
                if attrs
                    .get("encoding")
                    .is_some_and(|s| !s.eq_ignore_ascii_case("UTF-8"))
                    || attrs
                        .get("standalone")
                        .is_some_and(|s| s != "yes" && s != "no")
                {
                    return Err(refusal("unsupported encoding or standalone declaration"));
                }
            }
            Event::Comment(_) => {}
            Event::DocType(_) | Event::PI(_) => {
                return Err(refusal("DTD and processing instructions are unsupported"))
            }
            Event::Eof => {
                if !stack.is_empty() {
                    return Err(refusal("truncated document"));
                }
                return root.ok_or_else(|| refusal("missing root element"));
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// 15-113r6 §10.1.7: retain all supported Version declarations.
    #[test]
    fn r1x_version_meta_spec_shape_and_provenance() {
        let v = VersionXml::parse(r#"<Version><PreviousIncrementalRootDirectory name="../older"/><Comment>Demo</Comment><Specification version="1.2" authority="OGC" update="2"/><Metadata standard="DCAT"/><Extension name="custom" version="4"/></Version>"#).unwrap();
        assert_eq!(v.specification, Some(DeclaredVersion::V1_2));
        assert_eq!(v.specification_raw.as_deref(), Some("1.2"));
        assert_eq!(v.specification_authority.as_deref(), Some("OGC"));
        assert_eq!(v.specification_update.as_deref(), Some("2"));
        assert_eq!(v.previous_root.as_deref(), Some("../older"));
        assert_eq!(v.comment.as_deref(), Some("Demo"));
        assert_eq!(v.metadata_standard.as_deref(), Some("DCAT"));
        assert_eq!(
            v.extension,
            Some(ExtensionDeclaration {
                name: "custom".into(),
                version: "4".into()
            })
        );
    }

    /// §10.1.7 example vocabulary: recognize declarations without inventing unknowns.
    #[test]
    fn r1x_version_meta_declared_vocabulary() {
        for (raw, expected, pre_ogc) in [
            ("1.0", DeclaredVersion::V1_0, false),
            ("1.1", DeclaredVersion::V1_1, false),
            ("1.2", DeclaredVersion::V1_2, false),
            ("3.0", DeclaredVersion::V3_0, true),
            ("3.1", DeclaredVersion::V3_1, true),
            ("3.2", DeclaredVersion::V3_2, true),
        ] {
            let v = VersionXml::parse(&format!(
                r#"<Version><Specification version="{raw}"/></Version>"#
            ))
            .unwrap();
            assert_eq!(v.specification, Some(expected));
            assert_eq!(expected.as_str(), raw);
            assert_eq!(expected.is_pre_ogc(), pre_ogc);
        }
        for raw in ["", "9.9", " 1.2 ", "1.20"] {
            let v = VersionXml::parse(&format!(
                r#"<Version><Specification version="{raw}"/></Version>"#
            ))
            .unwrap();
            assert_eq!(v.specification, None);
            assert_eq!(v.specification_raw.as_deref(), Some(raw));
        }
    }

    /// §10.1.7 Comment-only example: no invented declaration or chain.
    #[test]
    fn r1x_version_meta_minimal_and_empty_comment() {
        let v = VersionXml::parse("<Version><Comment>x</Comment></Version>").unwrap();
        assert_eq!(v.comment.as_deref(), Some("x"));
        assert_eq!(v.specification_raw, None);
        assert_eq!(v.previous_root, None);
        assert_eq!(
            VersionXml::parse("<Version/>").unwrap(),
            VersionXml::default()
        );
        assert_eq!(
            VersionXml::parse("<Version><Comment/></Version>")
                .unwrap()
                .comment
                .as_deref(),
            Some("")
        );
    }

    /// XML entities and CDATA preserve split text and attribute provenance.
    #[test]
    fn r1x_version_meta_decodes_entities_cdata_and_unicode() {
        let v = VersionXml::parse(r#"<?xml version="1.0" encoding="UTF-8"?><!-- before --><Version><Comment>  A&amp;B<![CDATA[ <C> ]]><!-- split -->é&#x21;  </Comment><PreviousIncrementalRootDirectory name="../a&amp;b&#47;c"/><Specification version="1&#46;2" authority="O&amp;G" update="&quot;x&apos;"/><Metadata standard="D&amp;C"/><Extension name="a&amp;b" version="1&#46;0"/></Version><!-- after -->"#).unwrap();
        assert_eq!(v.comment.as_deref(), Some("  A&B <C> é!  "));
        assert_eq!(v.previous_root.as_deref(), Some("../a&b/c"));
        assert_eq!(v.specification, Some(DeclaredVersion::V1_2));
        assert_eq!(v.specification_authority.as_deref(), Some("O&G"));
        assert_eq!(v.specification_update.as_deref(), Some("\"x'"));
        assert_eq!(v.metadata_standard.as_deref(), Some("D&C"));
        assert_eq!(v.extension.unwrap().name, "a&b");
    }

    /// A present empty previous-root control is malformed, never absent.
    #[test]
    fn r1x_version_meta_refuses_empty_or_missing_previous_root() {
        for control in [
            "<PreviousIncrementalRootDirectory/>",
            "<PreviousIncrementalRootDirectory name=''/>",
            "<PreviousIncrementalRootDirectory name=' &#32; '/>",
        ] {
            assert!(
                VersionXml::parse(&format!("<Version>{control}</Version>")).is_err(),
                "{control}"
            );
        }
    }

    /// §3.2/§10.1.11: retain per-Version declarations for precedence decisions.
    #[test]
    fn r1x_version_meta_configuration_retains_order_and_declarations() {
        let c = ConfigurationXml::parse(r#"<Configuration><Comment>two versions</Comment><Version><Folder path="../a&amp;b"/><Specification version="1.1" authority="OGC" update="1"/><Comment>first</Comment><Metadata standard="DCAT"/><Extension name="ext" version="2"/></Version><Version><Folder path="../older"></Folder><Specification version="3.0"/></Version><Version><Folder path="../unknown"/><Specification version="9.9"/></Version><Version><Folder path="../undeclared"/></Version></Configuration>"#).unwrap();
        assert_eq!(c.comment.as_deref(), Some("two versions"));
        assert_eq!(
            c.version_folders,
            ["../a&b", "../older", "../unknown", "../undeclared"]
        );
        assert_eq!(c.versions.len(), 4);
        assert_eq!(c.versions[0].folder, "../a&b");
        let v = &c.versions[0].declaration;
        assert_eq!(v.specification, Some(DeclaredVersion::V1_1));
        assert_eq!(v.specification_authority.as_deref(), Some("OGC"));
        assert_eq!(v.specification_update.as_deref(), Some("1"));
        assert_eq!(v.comment.as_deref(), Some("first"));
        assert_eq!(v.metadata_standard.as_deref(), Some("DCAT"));
        assert_eq!(v.extension.as_ref().unwrap().version, "2");
        assert_eq!(
            c.versions[1].declaration.specification,
            Some(DeclaredVersion::V3_0)
        );
        assert_eq!(
            c.versions[2].declaration.specification_raw.as_deref(),
            Some("9.9")
        );
        assert_eq!(c.versions[3].declaration.specification_raw, None);
        assert!(ConfigurationXml::parse("<Configuration/>")
            .unwrap()
            .versions
            .is_empty());
    }

    /// Supported-subset refusal: nested/misplaced/unknown controls cannot vanish.
    #[test]
    fn r1x_version_meta_refuses_wrong_structure() {
        for xml in [
            "",
            "<Configuration/>",
            "<wrapper><Version/></wrapper>",
            "<Version><Wrapper><Specification version='1.2'/></Wrapper></Version>",
            "<Version><Comment><Specification version='1.2'/></Comment></Version>",
            "<Version><Folder path='x'/></Version>",
            "<Version><Version/></Version>",
            "<Version>unmodeled text</Version>",
            "<Version><Metadata standard='x'>text</Metadata></Version>",
            "<Version xmlns='urn:other'/>",
            "<Version extra='x'/>",
            "<Version><Specification version='1.2' extra='x'/></Version>",
        ] {
            assert!(VersionXml::parse(xml).is_err(), "{xml}");
        }
        for xml in ["<Version/>", "<Configuration><Folder path='x'/></Configuration>", "<Configuration><Specification version='1.2'/></Configuration>", "<Configuration><Version><Version><Folder path='x'/></Version></Version></Configuration>", "<Configuration><Version/></Configuration>", "<Configuration><Version><Folder/></Version></Configuration>", "<Configuration><Version><Folder path=' '/></Version></Configuration>", "<Configuration><Version><PreviousIncrementalRootDirectory name='x'/><Folder path='x'/></Version></Configuration>"] {
            assert!(ConfigurationXml::parse(xml).is_err(), "{xml}");
        }
    }

    /// Duplicate singleton controls/attributes cannot overwrite evidence.
    #[test]
    fn r1x_version_meta_refuses_duplicates() {
        for child in [
            "<Comment/>",
            "<Specification version='1.2'/>",
            "<Metadata standard='x'/>",
            "<Extension name='x' version='1'/>",
            "<PreviousIncrementalRootDirectory name='x'/>",
        ] {
            assert!(
                VersionXml::parse(&format!("<Version>{child}{child}</Version>")).is_err(),
                "{child}"
            );
        }
        for xml in [
            "<Version><Specification version='1.2' version='1.1'/></Version>",
            "<Version><Specification version='1.2' authority='a' authority='b'/></Version>",
            "<Version><Comment x='1' x='2'/></Version>",
        ] {
            assert!(VersionXml::parse(xml).is_err(), "{xml}");
        }
        for xml in ["<Configuration><Comment/><Comment/></Configuration>", "<Configuration><Version><Folder path='a'/><Folder path='b'/></Version></Configuration>", "<Configuration><Version><Folder path='a'/><Specification version='1.2'/><Specification version='1.1'/></Version></Configuration>"] {
            assert!(ConfigurationXml::parse(xml).is_err(), "{xml}");
        }
    }

    /// Refuse malformed documents, DTDs/entities and unsupported controls.
    #[test]
    fn r1x_version_meta_refuses_malformed_and_unsafe_xml() {
        for xml in ["<Version>", "<Version><Comment>x</Comment>", "<Version></Other>", "<Version/><Version/>", "before<Version/>", "<Version/>after", "<!DOCTYPE Version><Version/>", "<!DOCTYPE Version [<!ENTITY x '1.2'>]><Version><Specification version='&x;'/></Version>", "<Version><Comment>&unknown;</Comment></Version>", "<Version><Specification version='&unknown;'/></Version>", "<Version><Comment>&#0;</Comment></Version>", "<Version><Comment>\u{0000}</Comment></Version>", "<Version><Comment>bad ]]> text</Comment></Version>", "<Version><!-- bad -- comment --></Version>", "<?xml version='1.0'?><?xml version='1.0'?><Version/>", "<Version><?xml version='1.0'?></Version>", "<Version><?control value?></Version>", "<?xml version='1.1'?><Version/>", "<?xml version='1.0' encoding='ISO-8859-1'?><Version/>", "<Version><Specification/></Version>", "<Version><Metadata/></Version>", "<Version><Extension name='x'/></Version>", "<Version><Specification version='a<b'/></Version>"] {
            assert!(VersionXml::parse(xml).is_err(), "{xml:?}");
        }
        for xml in [
            "<Configuration>",
            "<Configuration/><Configuration/>",
            "<!DOCTYPE Configuration><Configuration/>",
            "<Configuration><Version><Folder path='&unknown;'/></Version></Configuration>",
        ] {
            assert!(ConfigurationXml::parse(xml).is_err(), "{xml}");
        }
    }
}

#[cfg(test)]
mod lexical_tests {
    use super::VersionXml;

    /// Refuse malformed declaration/attribute syntax that a pull reader may tolerate.
    #[test]
    fn r1x_version_meta_refuses_malformed_attribute_boundaries() {
        for xml in [
            "<Version><Specification version='1.2'authority='OGC'/></Version>",
            "<?xml version='1.0' standalone='yes' encoding='UTF-8'?><Version/>",
        ] {
            assert!(VersionXml::parse(xml).is_err(), "{xml}");
        }
    }
}
