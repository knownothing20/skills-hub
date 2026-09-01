#[cfg(not(windows))]
use std::fs::Metadata;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path};

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

const MAX_OPENAI_YAML_BYTES: u64 = 64 * 1024;
const MAX_ICON_BYTES: u64 = 128 * 1024;
const MAX_RASTER_DIMENSION: u32 = 512;
const MAX_RASTER_PIXELS: u64 = 262_144;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SkillUiMetadata {
    pub icon_data_url: Option<String>,
    pub brand_color: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct InterfaceFields {
    icon_small: Option<String>,
    icon_large: Option<String>,
    brand_color: Option<String>,
}

/// Read optional UI metadata from a Skill's standard `agents/openai.yaml`.
///
/// Invalid or unsafe cosmetic metadata never makes the Skill itself unusable.
/// Paths are resolved within the Skill root, converted to bounded data URLs,
/// and never exposed to the webview as arbitrary local filesystem paths.
pub fn read_skill_ui_metadata(skill_root: &Path) -> SkillUiMetadata {
    match try_read_skill_ui_metadata(skill_root) {
        Ok(metadata) => metadata,
        Err(err) => {
            let skill_label = skill_root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("<unknown>");
            log::warn!("ignored invalid UI metadata for Skill {skill_label}: {err:#}");
            SkillUiMetadata::default()
        }
    }
}

fn try_read_skill_ui_metadata(skill_root: &Path) -> Result<SkillUiMetadata> {
    let metadata_path = skill_root.join("agents").join("openai.yaml");
    let text_bytes = match read_bounded_regular_file(
        skill_root,
        &metadata_path,
        MAX_OPENAI_YAML_BYTES,
        "agents/openai.yaml",
    ) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SkillUiMetadata::default())
        }
        Err(err) => return Err(err).context("read agents/openai.yaml"),
    };
    let text = std::str::from_utf8(&text_bytes).context("agents/openai.yaml is not UTF-8")?;
    let fields = parse_interface_fields(text);
    let icon_data_url = [fields.icon_small.as_deref(), fields.icon_large.as_deref()]
        .into_iter()
        .flatten()
        .find_map(|path| match read_icon_data_url(skill_root, path) {
            Ok(data_url) => Some(data_url),
            Err(err) => {
                log::warn!("ignored invalid Skill icon reference: {err:#}");
                None
            }
        });

    Ok(SkillUiMetadata {
        icon_data_url,
        brand_color: fields.brand_color.filter(|color| is_brand_color(color)),
    })
}

fn parse_interface_fields(text: &str) -> InterfaceFields {
    let lines: Vec<&str> = text.lines().collect();
    let Some(interface_index) = lines.iter().position(|line| {
        let trimmed = line.trim();
        leading_spaces(line) == 0
            && trimmed
                .strip_prefix("interface:")
                .is_some_and(|tail| tail.trim().is_empty() || tail.trim().starts_with('#'))
    }) else {
        return InterfaceFields::default();
    };

    let section: Vec<&str> = lines
        .iter()
        .skip(interface_index + 1)
        .copied()
        .take_while(|line| {
            let trimmed = line.trim();
            trimmed.is_empty() || trimmed.starts_with('#') || leading_spaces(line) > 0
        })
        .collect();
    let Some(field_indent) = section
        .iter()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#') && trimmed.contains(':')
        })
        .map(|line| leading_spaces(line))
        .min()
    else {
        return InterfaceFields::default();
    };

    let mut fields = InterfaceFields::default();
    for line in section {
        if leading_spaces(line) != field_indent {
            continue;
        }
        let trimmed = line.trim();
        let Some((key, raw_value)) = trimmed.split_once(':') else {
            continue;
        };
        let value = parse_yaml_scalar(raw_value);
        match key.trim() {
            "icon_small" => fields.icon_small = value,
            "icon_large" => fields.icon_large = value,
            "brand_color" => fields.brand_color = value,
            _ => {}
        }
    }
    fields
}

fn leading_spaces(line: &str) -> usize {
    line.as_bytes()
        .iter()
        .take_while(|byte| **byte == b' ')
        .count()
}

fn parse_yaml_scalar(raw_value: &str) -> Option<String> {
    let value = strip_inline_comment(raw_value).trim();
    if value.is_empty() || matches!(value, "null" | "Null" | "NULL" | "~" | "|" | ">") {
        return None;
    }

    if value.starts_with('"') {
        return serde_json::from_str::<String>(value).ok();
    }
    if value.starts_with('\'') {
        if value.len() < 2 || !value.ends_with('\'') {
            return None;
        }
        return Some(value[1..value.len() - 1].replace("''", "'"));
    }
    Some(value.to_string())
}

fn strip_inline_comment(value: &str) -> &str {
    let mut single_quoted = false;
    let mut double_quoted = false;
    let mut escaped = false;
    for (index, ch) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && double_quoted {
            escaped = true;
            continue;
        }
        if ch == '\'' && !double_quoted {
            single_quoted = !single_quoted;
            continue;
        }
        if ch == '"' && !single_quoted {
            double_quoted = !double_quoted;
            continue;
        }
        if ch == '#' && !single_quoted && !double_quoted {
            return &value[..index];
        }
    }
    value
}

fn read_icon_data_url(skill_root: &Path, raw_path: &str) -> Result<String> {
    validate_relative_icon_path(raw_path)?;
    let candidate = skill_root.join(raw_path);
    let extension = candidate
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .context("Skill icon has no supported extension")?;
    let bytes = read_bounded_regular_file(skill_root, &candidate, MAX_ICON_BYTES, "Skill icon")
        .context("read Skill icon")?;
    let mime = validate_icon_bytes(&extension, &bytes)?;
    Ok(format!(
        "data:{mime};base64,{}",
        BASE64_STANDARD.encode(bytes)
    ))
}

/// Open and stream a file through a hard byte limit, then re-check the path and
/// file identity. The initial metadata length is only an early rejection; the
/// `take(limit + 1)` read is the actual resource boundary if a file grows or is
/// replaced while being read.
fn read_bounded_regular_file(
    skill_root: &Path,
    candidate: &Path,
    limit: u64,
    label: &str,
) -> std::io::Result<Vec<u8>> {
    let canonical_root = fs::canonicalize(skill_root)?;
    let before = fs::symlink_metadata(candidate)?;
    if before.file_type().is_symlink() || !before.is_file() {
        return Err(std::io::Error::other(format!(
            "{label} must be a regular non-symlink file"
        )));
    }
    if before.len() > limit {
        return Err(std::io::Error::other(format!(
            "{label} exceeds the size limit"
        )));
    }

    #[cfg(windows)]
    let initial_file = File::open(candidate)?;
    let canonical_before = fs::canonicalize(candidate)?;
    if !canonical_before.starts_with(&canonical_root) {
        return Err(std::io::Error::other(format!(
            "{label} resolves outside the Skill root"
        )));
    }

    let file = File::open(&canonical_before)?;
    let opened = file.metadata()?;
    if !opened.is_file() {
        return Err(std::io::Error::other(format!(
            "{label} is not a regular file"
        )));
    }
    #[cfg(unix)]
    if !same_file_identity(&before, &opened) {
        return Err(std::io::Error::other(format!(
            "{label} changed before it could be opened safely"
        )));
    }
    #[cfg(windows)]
    let opened_identity = {
        let initial_identity = windows_file_identity(&initial_file)?;
        let opened_identity = windows_file_identity(&file)?;
        if initial_identity != opened_identity {
            return Err(std::io::Error::other(format!(
                "{label} changed before it could be opened safely"
            )));
        }
        opened_identity
    };
    #[cfg(not(any(unix, windows)))]
    if !same_file_identity(&before, &opened) {
        return Err(std::io::Error::other(format!(
            "{label} changed before it could be opened safely"
        )));
    }

    let bytes = read_stream_bounded(file, limit, label)?;

    let after = fs::symlink_metadata(candidate)?;
    let canonical_after = fs::canonicalize(candidate)?;
    if after.file_type().is_symlink()
        || !after.is_file()
        || canonical_after != canonical_before
        || !canonical_after.starts_with(&canonical_root)
    {
        return Err(std::io::Error::other(format!(
            "{label} changed while it was being read"
        )));
    }
    #[cfg(unix)]
    if !same_file_identity(&opened, &after) {
        return Err(std::io::Error::other(format!(
            "{label} changed while it was being read"
        )));
    }
    #[cfg(windows)]
    {
        let after_file = File::open(candidate)?;
        if windows_file_identity(&after_file)? != opened_identity {
            return Err(std::io::Error::other(format!(
                "{label} changed while it was being read"
            )));
        }
    }
    #[cfg(not(any(unix, windows)))]
    if !same_file_identity(&opened, &after) {
        return Err(std::io::Error::other(format!(
            "{label} changed while it was being read"
        )));
    }

    Ok(bytes)
}

fn read_stream_bounded(reader: impl Read, limit: u64, label: &str) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::other(format!(
            "{label} exceeds the size limit"
        )));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn same_file_identity(left: &Metadata, right: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(windows)]
fn windows_file_identity(file: &File) -> std::io::Result<same_file::Handle> {
    same_file::Handle::from_file(file.try_clone()?)
}

#[cfg(not(any(unix, windows)))]
fn same_file_identity(left: &Metadata, right: &Metadata) -> bool {
    left.len() == right.len()
        && left.file_type() == right.file_type()
        && left.modified().ok() == right.modified().ok()
}

fn validate_relative_icon_path(raw_path: &str) -> Result<()> {
    if raw_path.is_empty()
        || raw_path != raw_path.trim()
        || raw_path.contains('\\')
        || raw_path.contains(':')
        || raw_path.chars().any(char::is_control)
        || raw_path.contains("://")
    {
        bail!("Skill icon path must be a clean relative path");
    }

    let path = Path::new(raw_path);
    if path.is_absolute() {
        bail!("Skill icon path must be relative");
    }
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("Skill icon path contains an unsafe component")
            }
        }
    }
    Ok(())
}

fn validate_icon_bytes(extension: &str, bytes: &[u8]) -> Result<&'static str> {
    match extension {
        "png" => {
            validate_raster_dimensions(parse_png_dimensions(bytes)?)?;
            Ok("image/png")
        }
        "jpg" | "jpeg" => {
            validate_raster_dimensions(parse_jpeg_dimensions(bytes)?)?;
            Ok("image/jpeg")
        }
        "webp" => {
            validate_raster_dimensions(parse_webp_dimensions(bytes)?)?;
            Ok("image/webp")
        }
        "svg" => {
            validate_svg(bytes)?;
            Ok("image/svg+xml")
        }
        _ => bail!("Skill icon extension and file signature do not match"),
    }
}

fn validate_raster_dimensions((width, height): (u32, u32)) -> Result<()> {
    if width == 0 || height == 0 {
        bail!("Skill raster icon has invalid dimensions");
    }
    if width > MAX_RASTER_DIMENSION || height > MAX_RASTER_DIMENSION {
        bail!("Skill raster icon dimensions exceed the limit");
    }
    if u64::from(width) * u64::from(height) > MAX_RASTER_PIXELS {
        bail!("Skill raster icon pixel count exceeds the limit");
    }
    Ok(())
}

fn parse_png_dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    if bytes.len() < 24
        || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || &bytes[12..16] != b"IHDR"
        || u32::from_be_bytes(bytes[8..12].try_into().unwrap()) != 13
    {
        bail!("Skill PNG has an invalid header");
    }
    Ok((
        u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
        u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
    ))
}

fn parse_jpeg_dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    if bytes.len() < 4 || !bytes.starts_with(&[0xff, 0xd8]) {
        bail!("Skill JPEG has an invalid header");
    }

    let mut cursor = 2usize;
    while cursor < bytes.len() {
        while cursor < bytes.len() && bytes[cursor] == 0xff {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            break;
        }
        let marker = bytes[cursor];
        cursor += 1;
        if marker == 0x00 || marker == 0xd8 || marker == 0xd9 || marker == 0x01 {
            continue;
        }
        if (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if cursor + 2 > bytes.len() {
            break;
        }
        let segment_len = usize::from(u16::from_be_bytes([bytes[cursor], bytes[cursor + 1]]));
        if segment_len < 2
            || match cursor.checked_add(segment_len) {
                Some(end) => end > bytes.len(),
                None => true,
            }
        {
            bail!("Skill JPEG contains an invalid segment");
        }

        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            if segment_len < 7 {
                bail!("Skill JPEG has an invalid frame header");
            }
            let height = u32::from(u16::from_be_bytes([bytes[cursor + 3], bytes[cursor + 4]]));
            let width = u32::from(u16::from_be_bytes([bytes[cursor + 5], bytes[cursor + 6]]));
            return Ok((width, height));
        }
        if marker == 0xda {
            break;
        }
        cursor += segment_len;
    }

    bail!("Skill JPEG has no supported frame dimensions")
}

fn parse_webp_dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    if bytes.len() < 20 || !bytes.starts_with(b"RIFF") || &bytes[8..12] != b"WEBP" {
        bail!("Skill WebP has an invalid header");
    }
    let riff_size = usize::try_from(u32::from_le_bytes(bytes[4..8].try_into().unwrap()))
        .context("Skill WebP RIFF size is invalid")?;
    if riff_size.checked_add(8) != Some(bytes.len()) {
        bail!("Skill WebP RIFF size does not match the file");
    }

    let mut cursor = 12usize;
    let mut dimensions = None;
    while cursor + 8 <= bytes.len() {
        let chunk_type = &bytes[cursor..cursor + 4];
        let chunk_size = usize::try_from(u32::from_le_bytes(
            bytes[cursor + 4..cursor + 8].try_into().unwrap(),
        ))
        .context("Skill WebP chunk size is invalid")?;
        let payload_start = cursor + 8;
        let payload_end = payload_start
            .checked_add(chunk_size)
            .context("Skill WebP chunk size overflow")?;
        if payload_end > bytes.len() {
            bail!("Skill WebP contains a truncated chunk");
        }
        let payload = &bytes[payload_start..payload_end];

        match chunk_type {
            b"VP8X" if payload.len() >= 10 => {
                if payload[0] & 0b0000_0010 != 0 {
                    bail!("animated Skill WebP icons are not supported");
                }
                let width = 1 + u32::from_le_bytes([payload[4], payload[5], payload[6], 0]);
                let height = 1 + u32::from_le_bytes([payload[7], payload[8], payload[9], 0]);
                dimensions.get_or_insert((width, height));
            }
            b"VP8L" if payload.len() >= 5 && payload[0] == 0x2f => {
                let bits = u32::from_le_bytes([payload[1], payload[2], payload[3], payload[4]]);
                dimensions.get_or_insert(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1));
            }
            b"VP8 " if payload.len() >= 10 && payload[3..6] == [0x9d, 0x01, 0x2a] => {
                let width = u32::from(u16::from_le_bytes([payload[6], payload[7]]) & 0x3fff);
                let height = u32::from(u16::from_le_bytes([payload[8], payload[9]]) & 0x3fff);
                dimensions.get_or_insert((width, height));
            }
            b"ANIM" | b"ANMF" => bail!("animated Skill WebP icons are not supported"),
            _ => {}
        }

        cursor = payload_end + (chunk_size & 1);
    }

    dimensions.context("Skill WebP has no supported image dimensions")
}

fn validate_svg(bytes: &[u8]) -> Result<()> {
    let svg = std::str::from_utf8(bytes).context("Skill SVG is not UTF-8")?;
    let svg = svg.trim_start_matches('\u{feff}');
    let mut reader = Reader::from_str(svg);
    reader.config_mut().check_end_names = true;
    reader.config_mut().allow_unmatched_ends = false;

    let mut stack: Vec<Vec<u8>> = Vec::new();
    let mut saw_root = false;
    let mut root_closed = false;
    loop {
        match reader.read_event().context("parse Skill SVG XML")? {
            Event::Start(element) => {
                validate_svg_start(&reader, &element, stack.is_empty(), saw_root, root_closed)?;
                if stack.is_empty() {
                    saw_root = true;
                }
                stack.push(element.name().as_ref().to_vec());
            }
            Event::Empty(element) => {
                validate_svg_start(&reader, &element, stack.is_empty(), saw_root, root_closed)?;
                if stack.is_empty() {
                    saw_root = true;
                    root_closed = true;
                }
            }
            Event::End(element) => {
                let Some(expected) = stack.pop() else {
                    bail!("Skill SVG contains an unmatched closing element");
                };
                if expected.as_slice() != element.name().as_ref() {
                    bail!("Skill SVG contains mismatched elements");
                }
                if stack.is_empty() {
                    root_closed = true;
                }
            }
            Event::Text(text) => {
                let decoded = text.decode().context("decode Skill SVG text")?;
                let unescaped =
                    quick_xml::escape::unescape(&decoded).context("unescape Skill SVG text")?;
                let text_allowed = stack
                    .last()
                    .is_some_and(|name| name.as_slice() == b"title" || name.as_slice() == b"desc");
                if !unescaped.trim().is_empty() && !text_allowed {
                    bail!("Skill SVG contains unsupported text content");
                }
            }
            Event::Decl(_) if !saw_root && stack.is_empty() => {}
            Event::Comment(_) => {}
            Event::Eof => break,
            Event::DocType(_)
            | Event::PI(_)
            | Event::CData(_)
            | Event::GeneralRef(_)
            | Event::Decl(_) => bail!("Skill SVG contains unsupported XML content"),
        }
    }

    if !saw_root || !root_closed || !stack.is_empty() {
        bail!("Skill SVG has no complete svg root");
    }
    Ok(())
}

fn validate_svg_start(
    reader: &Reader<&[u8]>,
    element: &BytesStart<'_>,
    top_level: bool,
    saw_root: bool,
    root_closed: bool,
) -> Result<()> {
    let element_name = element.name();
    let name = std::str::from_utf8(element_name.as_ref())
        .context("Skill SVG element name is not UTF-8")?;
    if top_level && (!name.eq("svg") || saw_root || root_closed) {
        bail!("Skill SVG must contain exactly one svg root");
    }
    if !is_allowed_svg_element(name) {
        bail!("Skill SVG element is not allowed: {name}");
    }

    for attribute in element.attributes().with_checks(true) {
        let attribute = attribute.context("parse Skill SVG attribute")?;
        let key = std::str::from_utf8(attribute.key.as_ref())
            .context("Skill SVG attribute name is not UTF-8")?;
        if key.to_ascii_lowercase().starts_with("on") || !is_allowed_svg_attribute(key) {
            bail!("Skill SVG attribute is not allowed: {key}");
        }
        let value = attribute
            .decode_and_unescape_value(reader.decoder())
            .context("decode Skill SVG attribute")?;
        validate_svg_attribute_value(key, &value)?;
    }
    Ok(())
}

fn is_allowed_svg_element(name: &str) -> bool {
    matches!(
        name,
        "svg"
            | "g"
            | "path"
            | "rect"
            | "circle"
            | "ellipse"
            | "line"
            | "polyline"
            | "polygon"
            | "defs"
            | "clipPath"
            | "mask"
            | "linearGradient"
            | "radialGradient"
            | "stop"
            | "title"
            | "desc"
    )
}

fn is_allowed_svg_attribute(name: &str) -> bool {
    matches!(
        name,
        "xmlns"
            | "version"
            | "id"
            | "viewBox"
            | "preserveAspectRatio"
            | "x"
            | "y"
            | "x1"
            | "y1"
            | "x2"
            | "y2"
            | "cx"
            | "cy"
            | "r"
            | "rx"
            | "ry"
            | "width"
            | "height"
            | "d"
            | "points"
            | "transform"
            | "fill"
            | "fill-opacity"
            | "fill-rule"
            | "stroke"
            | "stroke-width"
            | "stroke-linecap"
            | "stroke-linejoin"
            | "stroke-miterlimit"
            | "stroke-dasharray"
            | "stroke-dashoffset"
            | "stroke-opacity"
            | "opacity"
            | "clip-path"
            | "clip-rule"
            | "mask"
            | "maskUnits"
            | "maskContentUnits"
            | "gradientUnits"
            | "gradientTransform"
            | "spreadMethod"
            | "fx"
            | "fy"
            | "fr"
            | "offset"
            | "stop-color"
            | "stop-opacity"
            | "aria-hidden"
            | "aria-label"
            | "role"
            | "focusable"
            | "shape-rendering"
            | "vector-effect"
    )
}

fn validate_svg_attribute_value(name: &str, value: &str) -> Result<()> {
    if value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\t' | '\n' | '\r'))
    {
        bail!("Skill SVG attribute contains control characters");
    }
    if value.contains('\\') {
        bail!("Skill SVG attribute contains an unsupported escape");
    }
    if name == "xmlns" {
        if value != "http://www.w3.org/2000/svg" {
            bail!("Skill SVG namespace is not allowed");
        }
        return Ok(());
    }

    let normalized = value.trim().to_ascii_lowercase();
    if normalized.contains("javascript:")
        || normalized.contains("data:")
        || normalized.contains("file:")
        || normalized.contains("http:")
        || normalized.contains("https:")
        || normalized.starts_with("//")
    {
        bail!("Skill SVG contains a non-local reference");
    }

    if normalized.contains("url")
        && (!matches!(name, "fill" | "stroke" | "clip-path" | "mask")
            || !is_local_fragment_url(&normalized))
    {
        bail!("Skill SVG contains an unsafe URL reference");
    }
    Ok(())
}

fn is_local_fragment_url(value: &str) -> bool {
    let Some(fragment) = value
        .strip_prefix("url(#")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return false;
    };
    !fragment.is_empty()
        && fragment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

fn is_brand_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn write_skill(root: &Path, yaml: &str) {
        fs::create_dir_all(root.join("agents")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::write(root.join("agents/openai.yaml"), yaml).unwrap();
    }

    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR".to_vec();
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes
    }

    #[test]
    fn reads_standard_small_icon_and_brand_color() {
        let dir = tempfile::tempdir().unwrap();
        write_skill(
            dir.path(),
            "interface:\n  display_name: \"Example\"\n  icon_small: \"./assets/icon.svg\"\n  icon_large: \"./assets/large.png\"\n  brand_color: \"#3B82F6\"\n",
        );
        fs::write(
            dir.path().join("assets/icon.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 1\"><path d=\"M0 0\"/></svg>",
        )
        .unwrap();

        let metadata = read_skill_ui_metadata(dir.path());
        assert!(metadata
            .icon_data_url
            .as_deref()
            .unwrap()
            .starts_with("data:image/svg+xml;base64,"));
        assert_eq!(metadata.brand_color.as_deref(), Some("#3B82F6"));
    }

    #[test]
    fn falls_back_to_large_icon_when_small_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        write_skill(
            dir.path(),
            "interface:\n    icon_small: './assets/missing.svg'\n    icon_large: './assets/large.png' # fallback\n",
        );
        fs::write(dir.path().join("assets/large.png"), png_header(256, 256)).unwrap();

        let metadata = read_skill_ui_metadata(dir.path());
        assert!(metadata
            .icon_data_url
            .as_deref()
            .unwrap()
            .starts_with("data:image/png;base64,"));
    }

    #[test]
    fn falls_back_to_large_icon_when_small_svg_is_unsafe() {
        let dir = tempfile::tempdir().unwrap();
        write_skill(
            dir.path(),
            "interface:\n  icon_small: './assets/unsafe.svg'\n  icon_large: './assets/large.svg'\n",
        );
        fs::write(
            dir.path().join("assets/unsafe.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><script/></svg>",
        )
        .unwrap();
        fs::write(
            dir.path().join("assets/large.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 1\"><path d=\"M0 0\"/></svg>",
        )
        .unwrap();

        let metadata = read_skill_ui_metadata(dir.path());
        assert!(metadata
            .icon_data_url
            .as_deref()
            .unwrap()
            .starts_with("data:image/svg+xml;base64,"));
    }

    #[test]
    fn rejects_path_traversal_without_exposing_a_file() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("skill");
        write_skill(
            &root,
            "interface:\n  icon_small: \"../secret.png\"\n  brand_color: \"#abcdef\"\n",
        );
        fs::write(parent.path().join("secret.png"), b"\x89PNG\r\n\x1a\nsecret").unwrap();

        let metadata = read_skill_ui_metadata(&root);
        assert!(metadata.icon_data_url.is_none());
        assert_eq!(metadata.brand_color.as_deref(), Some("#abcdef"));
    }

    #[test]
    fn rejects_urls_and_windows_drive_paths() {
        for path in [
            "https://example.com/icon.png",
            "data:image/png;base64,AA==",
            "C:/Users/example/icon.png",
            r"\\server\share\icon.png",
        ] {
            assert!(
                validate_relative_icon_path(path).is_err(),
                "accepted {path}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_icon_even_when_it_points_inside_the_skill() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        write_skill(
            dir.path(),
            "interface:\n  icon_small: \"./assets/icon.svg\"\n",
        );
        fs::write(
            dir.path().join("assets/real.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
        )
        .unwrap();
        symlink("real.svg", dir.path().join("assets/icon.svg")).unwrap();

        assert!(read_skill_ui_metadata(dir.path()).icon_data_url.is_none());
    }

    #[test]
    fn rejects_active_svg_content() {
        let dir = tempfile::tempdir().unwrap();
        write_skill(
            dir.path(),
            "interface:\n  icon_small: \"./assets/icon.svg\"\n",
        );
        fs::write(
            dir.path().join("assets/icon.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>",
        )
        .unwrap();

        assert!(read_skill_ui_metadata(dir.path()).icon_data_url.is_none());
    }

    #[test]
    fn rejects_svg_animation_and_reference_elements() {
        for element in [
            "<animate attributeName=\"opacity\"/>",
            "<set attributeName=\"fill\" to=\"red\"/>",
            "<use href=\"#shape\"/>",
            "<image href=\"icon.png\"/>",
            "<foreignObject/>",
        ] {
            let svg = format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 1\">{element}</svg>"
            );
            assert!(validate_svg(svg.as_bytes()).is_err(), "accepted {element}");
        }
    }

    #[test]
    fn rejects_numeric_character_and_external_url_references() {
        for svg in [
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><use href=\"&#106;avascript:alert(1)\"/></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path fill=\"url(&#104;ttps://example.com/a.svg#x)\" d=\"M0 0\"/></svg>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path fill=\"url(//example.com/a.svg#x)\" d=\"M0 0\"/></svg>",
            r#"<svg xmlns="http://www.w3.org/2000/svg"><path fill="u\72l(https://example.com/a.svg#x)" d="M0 0"/></svg>"#,
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><path style=\"fill:url(#x)\" d=\"M0 0\"/></svg>",
        ] {
            assert!(validate_svg(svg.as_bytes()).is_err(), "accepted {svg}");
        }
    }

    #[test]
    fn permits_static_svg_with_local_gradient_reference() {
        let svg = concat!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\" aria-hidden=\"true\">",
            "<defs><linearGradient id=\"paint\"><stop offset=\"0\" stop-color=\"#fff\"/>",
            "</linearGradient></defs><path fill=\"url(#paint)\" d=\"M0 0h10v10z\"/></svg>"
        );
        validate_svg(svg.as_bytes()).unwrap();
    }

    #[test]
    fn rejects_dtd_entities_and_stylesheets() {
        for svg in [
            "<!DOCTYPE svg [<!ENTITY xxe SYSTEM \"file:///etc/passwd\">]><svg>&xxe;</svg>",
            "<?xml-stylesheet href=\"https://example.com/a.css\"?><svg/>",
        ] {
            assert!(validate_svg(svg.as_bytes()).is_err(), "accepted {svg}");
        }
    }

    #[test]
    fn stream_limit_rejects_more_bytes_even_without_metadata_size_check() {
        let input = vec![b'x'; 17];
        let error = read_stream_bounded(Cursor::new(input), 16, "test input").unwrap_err();
        assert!(error.to_string().contains("exceeds the size limit"));
        assert_eq!(
            read_stream_bounded(Cursor::new(vec![b'x'; 16]), 16, "test input")
                .unwrap()
                .len(),
            16
        );
    }

    #[test]
    fn rejects_oversized_metadata_and_icon_files() {
        let dir = tempfile::tempdir().unwrap();
        write_skill(
            dir.path(),
            &format!(
                "interface:\n  icon_small: './assets/icon.svg'\n#{}",
                "x".repeat(MAX_OPENAI_YAML_BYTES as usize)
            ),
        );
        assert_eq!(
            read_skill_ui_metadata(dir.path()),
            SkillUiMetadata::default()
        );

        fs::write(
            dir.path().join("agents/openai.yaml"),
            "interface:\n  icon_small: './assets/icon.svg'\n",
        )
        .unwrap();
        let mut icon = b"<svg xmlns=\"http://www.w3.org/2000/svg\">".to_vec();
        icon.extend(std::iter::repeat_n(b' ', MAX_ICON_BYTES as usize));
        icon.extend_from_slice(b"</svg>");
        fs::write(dir.path().join("assets/icon.svg"), icon).unwrap();
        assert!(read_skill_ui_metadata(dir.path()).icon_data_url.is_none());
    }

    #[test]
    fn rejects_giant_raster_dimensions() {
        assert!(validate_icon_bytes("png", &png_header(512, 512)).is_ok());
        assert!(validate_icon_bytes("png", &png_header(513, 512)).is_err());
        assert!(validate_icon_bytes("png", &png_header(512, 513)).is_err());
    }

    #[test]
    fn parses_jpeg_and_webp_dimensions_before_accepting() {
        let jpeg = [
            0xff, 0xd8, 0xff, 0xc0, 0x00, 0x07, 0x08, 0x01, 0x00, 0x02, 0x00,
        ];
        assert_eq!(parse_jpeg_dimensions(&jpeg).unwrap(), (512, 256));

        let mut webp = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0".to_vec();
        webp.extend_from_slice(&[0, 0, 0, 0, 0xff, 0x00, 0x00, 0xff, 0x00, 0x00]);
        let riff_size = u32::try_from(webp.len() - 8).unwrap();
        webp[4..8].copy_from_slice(&riff_size.to_le_bytes());
        assert_eq!(parse_webp_dimensions(&webp).unwrap(), (256, 256));
    }

    #[test]
    fn ignores_nested_keys_and_invalid_brand_color() {
        let fields = parse_interface_fields(
            "interface:\n  nested:\n    icon_small: \"./wrong.svg\"\n  brand_color: \"red\"\n",
        );
        assert!(fields.icon_small.is_none());
        assert!(!is_brand_color(fields.brand_color.as_deref().unwrap()));
    }
}
