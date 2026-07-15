//! Minimal WKT-2 (ISO 19162) reader for CRS metadata.
//!
//! Requirement CRS5 mandates WKT-2 as the CRS metadata encoding; this is a
//! purpose-built structural parser — keyword tree, quoted strings with `""`
//! escapes, numbers, bare enumerations — not a full semantic WKT-2
//! implementation. It is deliberately lenient about missing commas between
//! sibling nodes and whitespace before brackets, because the spec's own CRS
//! examples (§7.3.1.4, §7.3.1.9) contain both.

use std::fmt;

use super::CrsViolation;

const MAX_DEPTH: usize = 64;

/// A WKT node: `KEYWORD[argument, …]`. Keywords are canonicalized to
/// uppercase.
#[derive(Debug, Clone, PartialEq)]
pub struct WktNode {
    pub keyword: String,
    pub arguments: Vec<WktValue>,
}

/// An argument of a WKT node.
#[derive(Debug, Clone, PartialEq)]
pub enum WktValue {
    /// A quoted string, unescaped.
    Text(String),
    Number(f64),
    /// A bare enumeration keyword such as `north` or `ellipsoidal`.
    Keyword(String),
    Node(WktNode),
}

impl WktNode {
    /// The node's name: its first quoted-string argument.
    pub fn name(&self) -> Option<&str> {
        self.arguments.iter().find_map(|argument| match argument {
            WktValue::Text(text) => Some(text.as_str()),
            _ => None,
        })
    }

    pub fn first_number(&self) -> Option<f64> {
        self.arguments.iter().find_map(|argument| match argument {
            WktValue::Number(number) => Some(*number),
            _ => None,
        })
    }

    pub fn child_nodes(&self) -> impl Iterator<Item = &WktNode> {
        self.arguments.iter().filter_map(|argument| match argument {
            WktValue::Node(node) => Some(node),
            _ => None,
        })
    }

    /// First direct child node with the given (uppercase) keyword.
    pub fn find(&self, keyword: &str) -> Option<&WktNode> {
        self.child_nodes().find(|node| node.keyword == keyword)
    }
}

impl fmt::Display for WktNode {
    /// Canonical single-line form: `KEYWORD[a,b,…]`, quotes doubled.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[", self.keyword)?;
        for (index, argument) in self.arguments.iter().enumerate() {
            if index > 0 {
                f.write_str(",")?;
            }
            write!(f, "{argument}")?;
        }
        f.write_str("]")
    }
}

impl fmt::Display for WktValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WktValue::Text(text) => write!(f, "\"{}\"", text.replace('"', "\"\"")),
            WktValue::Number(number) => write!(f, "{number}"),
            WktValue::Keyword(keyword) => f.write_str(keyword),
            WktValue::Node(node) => write!(f, "{node}"),
        }
    }
}

/// Parses one WKT-2 definition; trailing content is an error.
pub fn parse(input: &str) -> Result<WktNode, CrsViolation> {
    let mut parser = Parser { input, pos: 0 };
    parser.skip_ws();
    let node = parser.parse_node(0)?;
    parser.skip_ws();
    if parser.pos != input.len() {
        return Err(parser.err("unexpected trailing content"));
    }
    Ok(node)
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b) if b.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn err(&self, reason: impl Into<String>) -> CrsViolation {
        CrsViolation::InvalidWkt {
            position: self.pos,
            reason: reason.into(),
        }
    }

    fn parse_keyword(&mut self) -> Result<String, CrsViolation> {
        let start = self.pos;
        match self.peek() {
            Some(b) if b.is_ascii_alphabetic() => self.pos += 1,
            _ => return Err(self.err("expected a WKT keyword")),
        }
        while matches!(self.peek(), Some(b) if b.is_ascii_alphanumeric() || b == b'_') {
            self.pos += 1;
        }
        Ok(self.input[start..self.pos].to_ascii_uppercase())
    }

    fn parse_node(&mut self, depth: usize) -> Result<WktNode, CrsViolation> {
        if depth > MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        let keyword = self.parse_keyword()?;
        self.skip_ws();
        let close = match self.peek() {
            Some(b'[') => b']',
            Some(b'(') => b')',
            _ => return Err(self.err(format!("expected `[` after {keyword}"))),
        };
        self.pos += 1;
        let mut arguments = Vec::new();
        self.skip_ws();
        if self.peek() == Some(close) {
            self.pos += 1;
            return Ok(WktNode { keyword, arguments });
        }
        loop {
            self.skip_ws();
            arguments.push(self.parse_value(depth)?);
            self.skip_ws();
            match self.peek() {
                Some(b) if b == close => {
                    self.pos += 1;
                    break;
                }
                Some(b',') => {
                    self.pos += 1;
                }
                Some(b']' | b')') => return Err(self.err("mismatched closing bracket")),
                None => return Err(self.err(format!("unterminated {keyword}"))),
                // Lenient: a new argument without a separating comma (the
                // spec's compound example omits one between member CRSs).
                Some(_) => {}
            }
        }
        Ok(WktNode { keyword, arguments })
    }

    fn parse_value(&mut self, depth: usize) -> Result<WktValue, CrsViolation> {
        match self.peek() {
            Some(b'"') => self.parse_string().map(WktValue::Text),
            Some(b) if b == b'-' || b == b'+' || b == b'.' || b.is_ascii_digit() => {
                self.parse_number()
            }
            Some(b) if b.is_ascii_alphabetic() => {
                let start = self.pos;
                let keyword = self.parse_keyword()?;
                let after_keyword = self.pos;
                self.skip_ws();
                if matches!(self.peek(), Some(b'[' | b'(')) {
                    self.pos = start;
                    self.parse_node(depth + 1).map(WktValue::Node)
                } else {
                    self.pos = after_keyword;
                    Ok(WktValue::Keyword(keyword))
                }
            }
            Some(_) => Err(self.err("unexpected character")),
            None => Err(self.err("unexpected end of input")),
        }
    }

    fn parse_string(&mut self) -> Result<String, CrsViolation> {
        self.pos += 1; // opening quote
        let mut out = String::new();
        let mut segment_start = self.pos;
        loop {
            match self.peek() {
                None => return Err(self.err("unterminated quoted string")),
                Some(b'"') => {
                    out.push_str(&self.input[segment_start..self.pos]);
                    self.pos += 1;
                    if self.peek() == Some(b'"') {
                        out.push('"');
                        self.pos += 1;
                        segment_start = self.pos;
                    } else {
                        return Ok(out);
                    }
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    fn parse_number(&mut self) -> Result<WktValue, CrsViolation> {
        let start = self.pos;
        while matches!(
            self.peek(),
            Some(b) if b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E')
        ) {
            self.pos += 1;
        }
        let text = &self.input[start..self.pos];
        text.parse::<f64>()
            .map(WktValue::Number)
            .map_err(|_| CrsViolation::InvalidWkt {
                position: start,
                reason: format!("invalid number {text:?}"),
            })
    }
}

/// CRS classification per the WKT-2 root keyword (2015 and 2019 aliases).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrsKind {
    Geodetic,
    Geographic,
    Projected,
    Vertical,
    Engineering,
    Compound,
    Temporal,
    Parametric,
    Other(String),
}

pub fn classify(node: &WktNode) -> CrsKind {
    match node.keyword.as_str() {
        "GEODCRS" | "GEODETICCRS" => CrsKind::Geodetic,
        "GEOGCRS" | "GEOGRAPHICCRS" => CrsKind::Geographic,
        "PROJCRS" | "PROJECTEDCRS" => CrsKind::Projected,
        "VERTCRS" | "VERTICALCRS" => CrsKind::Vertical,
        "ENGCRS" | "ENGINEERINGCRS" => CrsKind::Engineering,
        "COMPOUNDCRS" => CrsKind::Compound,
        "TIMECRS" => CrsKind::Temporal,
        "PARAMETRICCRS" => CrsKind::Parametric,
        other => CrsKind::Other(other.to_owned()),
    }
}

pub fn is_crs_node(node: &WktNode) -> bool {
    !matches!(classify(node), CrsKind::Other(_))
}

/// The `ID[...]`/`AUTHORITY[...]` of a node as (authority, code).
pub fn authority_id(node: &WktNode) -> Option<(String, String)> {
    let id = node.find("ID").or_else(|| node.find("AUTHORITY"))?;
    let mut values = id.arguments.iter();
    let authority = match values.next()? {
        WktValue::Text(text) => text.clone(),
        _ => return None,
    };
    let code = match values.next()? {
        WktValue::Text(text) => text.clone(),
        WktValue::Number(number) if number.fract() == 0.0 && number.abs() < 9e15 => {
            format!("{}", *number as i64)
        }
        WktValue::Number(number) => format!("{number}"),
        _ => return None,
    };
    Some((authority, code))
}

/// Whether a CRS node declares a dynamic reference frame (`DYNAMIC[...]`).
pub fn is_dynamic(crs: &WktNode) -> bool {
    crs.find("DYNAMIC").is_some()
}

/// The frame reference epoch of a dynamic CRS, if stated.
pub fn frame_epoch(crs: &WktNode) -> Option<f64> {
    crs.find("DYNAMIC")?.find("FRAMEEPOCH")?.first_number()
}

/// The dimension a unit measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitKind {
    Angle,
    Length,
    Scale,
    Time,
    Parametric,
    Generic,
}

/// A WKT unit: keyword kind, name, and conversion factor.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitSpec {
    pub kind: UnitKind,
    pub name: String,
    pub factor: Option<f64>,
}

impl UnitSpec {
    /// The VCRS3 default: vertical extents are meters when unstated.
    pub fn metre() -> UnitSpec {
        UnitSpec {
            kind: UnitKind::Length,
            name: "metre".to_owned(),
            factor: Some(1.0),
        }
    }
}

impl fmt::Display for UnitSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.factor {
            Some(factor) => write!(f, "{} (factor {factor})", self.name),
            None => f.write_str(&self.name),
        }
    }
}

/// If `node` is a unit node, its [`UnitSpec`].
pub fn unit_of(node: &WktNode) -> Option<UnitSpec> {
    let kind = match node.keyword.as_str() {
        "ANGLEUNIT" => UnitKind::Angle,
        "LENGTHUNIT" => UnitKind::Length,
        "SCALEUNIT" => UnitKind::Scale,
        "TIMEUNIT" => UnitKind::Time,
        "PARAMETRICUNIT" => UnitKind::Parametric,
        "UNIT" => UnitKind::Generic,
        _ => return None,
    };
    Some(UnitSpec {
        kind,
        name: node.name().unwrap_or_default().to_owned(),
        factor: node.first_number(),
    })
}

/// The units governing a CRS component's coordinates: unit nodes that are
/// direct children of the CRS node (the common-unit form) or children of
/// its `AXIS` nodes. Units inside defining parameters (`ELLIPSOID`,
/// `PRIMEM`, …) do not govern coordinates and are excluded.
pub fn coordinate_units(crs: &WktNode) -> Vec<UnitSpec> {
    let mut units: Vec<UnitSpec> = crs.child_nodes().filter_map(unit_of).collect();
    for axis in crs.child_nodes().filter(|node| node.keyword == "AXIS") {
        units.extend(axis.child_nodes().filter_map(unit_of));
    }
    units
}

fn units_compatible(a: &UnitSpec, b: &UnitSpec) -> bool {
    if a.name.eq_ignore_ascii_case(&b.name) {
        return true;
    }
    match (a.factor, b.factor) {
        (Some(x), Some(y)) => {
            let scale = x.abs().max(y.abs()).max(1.0);
            (x - y).abs() <= 1e-9 * scale
        }
        _ => false,
    }
}

/// Requirement CRS6 (`/req/core/crs/uom`): within one CRS component, all
/// coordinate units of the same kind must agree.
pub fn check_coordinate_units(component: &WktNode) -> Result<(), CrsViolation> {
    let mut seen: Vec<UnitSpec> = Vec::new();
    for unit in coordinate_units(component) {
        if let Some(previous) = seen.iter().find(|candidate| candidate.kind == unit.kind) {
            if !units_compatible(previous, &unit) {
                return Err(CrsViolation::InconsistentCoordinateUnits {
                    component: component.name().unwrap_or_default().to_owned(),
                    first: previous.to_string(),
                    second: unit.to_string(),
                });
            }
        } else {
            seen.push(unit);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wkt2_parses_nested_structure() {
        let node = parse(r#"AXIS["latitude",north,ORDER[1]]"#).unwrap();
        assert_eq!(node.keyword, "AXIS");
        assert_eq!(node.name(), Some("latitude"));
        assert_eq!(node.arguments[1], WktValue::Keyword("NORTH".into()));
        assert_eq!(node.find("ORDER").unwrap().first_number(), Some(1.0));
    }

    #[test]
    fn wkt2_unescapes_doubled_quotes() {
        let node = parse(r#"REMARK["say ""hi"""]"#).unwrap();
        assert_eq!(node.name(), Some(r#"say "hi""#));
    }

    /// The spec's own examples omit commas between sibling nodes and put
    /// whitespace before brackets; both are accepted.
    #[test]
    fn wkt2_lenient_missing_commas_and_spaced_brackets() {
        let node = parse(r#"COMPOUNDCRS ["x" GEOGCRS["a"] VERTCRS["b"]]"#).unwrap();
        assert_eq!(node.keyword, "COMPOUNDCRS");
        assert_eq!(node.child_nodes().count(), 2);
    }

    #[test]
    fn wkt2_rejects_malformed() {
        for input in [
            "",
            "GEOGCRS",
            r#"GEOGCRS["x""#,
            r#"GEOGCRS["x"] extra"#,
            "ORDER[1.2.3]",
            r#"A[1)"#,
            r#"NAME["unterminated]"#,
        ] {
            assert!(
                matches!(parse(input), Err(CrsViolation::InvalidWkt { .. })),
                "{input:?}"
            );
        }
    }

    #[test]
    fn wkt2_depth_limit_guards_recursion() {
        let mut deep = String::new();
        for _ in 0..100 {
            deep.push_str("A[");
        }
        deep.push('1');
        deep.push_str(&"]".repeat(100));
        assert!(matches!(parse(&deep), Err(CrsViolation::InvalidWkt { .. })));
    }

    #[test]
    fn wkt2_classifies_crs_kinds_with_aliases() {
        let cases = [
            (r#"GEODCRS["x"]"#, CrsKind::Geodetic),
            (r#"GEOGRAPHICCRS["x"]"#, CrsKind::Geographic),
            (r#"PROJCRS["x"]"#, CrsKind::Projected),
            (r#"VERTICALCRS["x"]"#, CrsKind::Vertical),
            (r#"ENGCRS["x"]"#, CrsKind::Engineering),
            (r#"COMPOUNDCRS["x"]"#, CrsKind::Compound),
            (r#"DATUM["x"]"#, CrsKind::Other("DATUM".into())),
        ];
        for (input, expected) in cases {
            assert_eq!(classify(&parse(input).unwrap()), expected, "{input}");
        }
    }

    #[test]
    fn wkt2_canonical_display_roundtrips() {
        let input = r#"GEOGCRS["NTF (Paris)",
            DATUM["D", ELLIPSOID["Clarke 1880 (IGN)",6378249.2,293.4660213]],
            REMARK["Nouvelle Triangulation Française"]]"#;
        let node = parse(input).unwrap();
        let canonical = node.to_string();
        assert_eq!(parse(&canonical).unwrap(), node);
        assert!(canonical.contains(r#"ELLIPSOID["Clarke 1880 (IGN)",6378249.2,293.4660213]"#));
    }

    #[test]
    fn wkt2_extracts_authority_and_coordinate_units() {
        let wgs84 = parse(
            r#"GEODCRS["WGS 84",
              DATUM["World Geodetic System 1984",
                ELLIPSOID["WGS 84",6378137,298.257223563,LENGTHUNIT["metre",1.0]]],
              CS[ellipsoidal,2],
                AXIS["latitude",north,ORDER[1]],
                AXIS["longitude",east,ORDER[2]],
                ANGLEUNIT["degree",0.01745329252],
              ID["EPSG",4326]]"#,
        )
        .unwrap();
        assert_eq!(authority_id(&wgs84), Some(("EPSG".into(), "4326".into())));
        // Coordinate units: only the CRS-level ANGLEUNIT; the ellipsoid's
        // defining-parameter LENGTHUNIT is excluded.
        let units = coordinate_units(&wgs84);
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].kind, UnitKind::Angle);
        assert_eq!(units[0].name, "degree");
        assert!(check_coordinate_units(&wgs84).is_ok());
    }
}
