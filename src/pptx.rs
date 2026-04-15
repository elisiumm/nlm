// pptx.rs — Brand charter extraction from a PowerPoint template.
//
// A .pptx is a ZIP archive containing XML (OOXML — ECMA-376) plus media files.
// We only read it; we never write back. Files of interest:
//
//   ppt/theme/theme1.xml             — color scheme + font scheme
//   ppt/slides/slide{N}.xml          — slide content (text in <a:t> nodes)
//   ppt/slides/_rels/slide{N}.xml.rels — image refs per slide
//   ppt/slideLayouts/slideLayout{N}.xml — layout names
//   ppt/media/image{N}.{png,jpeg,svg} — extracted as assets
//
// Parsing strategy: we use quick-xml's pull parser (event loop) instead of
// full deserialization because OOXML's namespace + attribute soup makes serde
// derive painful for the few values we actually need.
//
// Rust concepts:
//   - quick_xml::events::Event for streaming XML
//   - std::io::Read trait for both file and zip-entry reads
//   - Pattern matching on Event variants for stateful parsing
//   - HashMap for relationship id → target lookup

use anyhow::{Context, Result};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use zip::ZipArchive;

// ── Public types ──────────────────────────────────────────────────────────────

/// Structured snapshot of everything we extract from a PPTX template.
pub struct BrandCharter {
    pub source_filename: String,
    pub theme_name: Option<String>,
    pub colors: Vec<ThemeColor>,
    pub major_font: Option<String>, // headings
    pub minor_font: Option<String>, // body
    pub layouts: Vec<String>,       // slide-layout type names (title, content, etc.)
    pub slides: Vec<SlideContent>,
    pub assets: Vec<ExtractedAsset>,
}

pub struct ThemeColor {
    pub role: String, // "accent1", "dk1", "lt1", "hlink", …
    pub hex: String,  // "5B9BD5"
}

pub struct SlideContent {
    pub index: usize,        // 1-based
    pub title: String,       // first text frame, best-effort
    pub body: String,        // remaining text, joined with newlines
    pub images: Vec<String>, // image filenames referenced by this slide
}

pub struct ExtractedAsset {
    pub original_name: String, // "image1.png"
    pub written_to: PathBuf,   // absolute path on disk
}

// ── Top-level extract ─────────────────────────────────────────────────────────

/// Parse a `.pptx` and write a brand-charter markdown to `<output_dir>/<stem>.md`,
/// plus extracted images under `<output_dir>/assets/`.
///
/// `dry_run` skips both the markdown write and the asset extraction; the parse
/// still runs end-to-end so the caller can inspect the returned `BrandCharter`.
pub fn import_pptx(
    pptx_path: &Path,
    output_dir: &Path,
    dry_run: bool,
) -> Result<(BrandCharter, Option<PathBuf>)> {
    let stem = pptx_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("brand")
        .to_string();

    let file =
        File::open(pptx_path).with_context(|| format!("Cannot open {}", pptx_path.display()))?;
    let mut zip = ZipArchive::new(file)
        .with_context(|| format!("{} is not a valid .pptx (zip) file", pptx_path.display()))?;

    let theme_xml = read_zip_string(&mut zip, "ppt/theme/theme1.xml").ok();
    let (theme_name, colors, major_font, minor_font) = match theme_xml.as_deref() {
        Some(xml) => parse_theme(xml).unwrap_or_default(),
        None => Default::default(),
    };

    let layouts = list_layouts(&mut zip);
    let slide_count = count_slides(&mut zip);

    let mut slides = Vec::with_capacity(slide_count);
    for i in 1..=slide_count {
        match read_slide(&mut zip, i) {
            Ok(s) => slides.push(s),
            Err(e) => slides.push(SlideContent {
                index: i,
                title: String::new(),
                body: format!("[unreadable: {e}]"),
                images: vec![],
            }),
        }
    }

    let assets_dir = output_dir.join("assets");
    let assets = if dry_run {
        list_media(&mut zip)
            .into_iter()
            .map(|name| ExtractedAsset {
                written_to: assets_dir.join(file_basename(&name)),
                original_name: name,
            })
            .collect()
    } else {
        std::fs::create_dir_all(&assets_dir)
            .with_context(|| format!("Cannot create {}", assets_dir.display()))?;
        extract_media(&mut zip, &assets_dir)?
    };

    let charter = BrandCharter {
        source_filename: pptx_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("(unknown)")
            .to_string(),
        theme_name,
        colors,
        major_font,
        minor_font,
        layouts,
        slides,
        assets,
    };

    let md_path = if dry_run {
        None
    } else {
        std::fs::create_dir_all(output_dir)
            .with_context(|| format!("Cannot create {}", output_dir.display()))?;
        let path = output_dir.join(format!("{stem}.md"));
        let md = render_markdown(&charter);
        std::fs::write(&path, md).with_context(|| format!("Cannot write {}", path.display()))?;
        Some(path)
    };

    Ok((charter, md_path))
}

// ── Zip helpers ───────────────────────────────────────────────────────────────

fn read_zip_string<R: Read + std::io::Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<String> {
    let mut entry = zip
        .by_name(name)
        .with_context(|| format!("{name} not found in archive"))?;
    let mut buf = String::new();
    entry.read_to_string(&mut buf)?;
    Ok(buf)
}

fn read_zip_bytes<R: Read + std::io::Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<Vec<u8>> {
    let mut entry = zip
        .by_name(name)
        .with_context(|| format!("{name} not found in archive"))?;
    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut buf)?;
    Ok(buf)
}

/// Enumerate slide N count by scanning `ppt/slides/slide*.xml` filenames.
fn count_slides<R: Read + std::io::Seek>(zip: &mut ZipArchive<R>) -> usize {
    let mut max = 0usize;
    for i in 0..zip.len() {
        let Ok(entry) = zip.by_index(i) else { continue };
        let name = entry.name();
        if let Some(num) = name
            .strip_prefix("ppt/slides/slide")
            .and_then(|s| s.strip_suffix(".xml"))
        {
            if let Ok(n) = num.parse::<usize>() {
                if n > max {
                    max = n;
                }
            }
        }
    }
    max
}

fn list_layouts<R: Read + std::io::Seek>(zip: &mut ZipArchive<R>) -> Vec<String> {
    let mut out = Vec::new();
    let mut indexes = Vec::new();
    for i in 0..zip.len() {
        let Ok(entry) = zip.by_index(i) else { continue };
        let name = entry.name();
        if name.starts_with("ppt/slideLayouts/slideLayout") && name.ends_with(".xml") {
            if let Ok(num) = name
                .trim_start_matches("ppt/slideLayouts/slideLayout")
                .trim_end_matches(".xml")
                .parse::<usize>()
            {
                indexes.push((num, name.to_string()));
            }
        }
    }
    indexes.sort_by_key(|(n, _)| *n);
    for (_, name) in indexes {
        if let Ok(xml) = read_zip_string(zip, &name) {
            out.push(extract_layout_name(&xml).unwrap_or_else(|| name.clone()));
        }
    }
    out
}

fn list_media<R: Read + std::io::Seek>(zip: &mut ZipArchive<R>) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let Ok(entry) = zip.by_index(i) else { continue };
        let name = entry.name();
        if name.starts_with("ppt/media/") {
            out.push(name.to_string());
        }
    }
    out
}

fn extract_media<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    target_dir: &Path,
) -> Result<Vec<ExtractedAsset>> {
    let names = list_media(zip);
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let basename = file_basename(&name);
        let dest = target_dir.join(&basename);
        let bytes = read_zip_bytes(zip, &name)?;
        let mut f =
            File::create(&dest).with_context(|| format!("Cannot create {}", dest.display()))?;
        f.write_all(&bytes)?;
        out.push(ExtractedAsset {
            original_name: name,
            written_to: dest,
        });
    }
    Ok(out)
}

fn file_basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

// ── Theme parsing ─────────────────────────────────────────────────────────────

/// Pull the color scheme + font scheme + scheme name out of `theme1.xml`.
///
/// Color slots in OOXML order: dk1, lt1, dk2, lt2, accent1..6, hlink, folHlink.
/// Each slot wraps one of `a:srgbClr val="HEX"` (literal) or `a:sysClr val="..."
/// lastClr="HEX"` (system color with a cached resolved hex).
/// (theme_name, colors, major_font, minor_font)
type ParsedTheme = (
    Option<String>,
    Vec<ThemeColor>,
    Option<String>,
    Option<String>,
);

fn parse_theme(xml: &str) -> Result<ParsedTheme> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut theme_name: Option<String> = None;
    let mut colors: Vec<ThemeColor> = Vec::new();
    let mut major_font: Option<String> = None;
    let mut minor_font: Option<String> = None;

    // Stack of currently-open (local-name, captured?) elements so we can
    // attribute child <a:srgbClr> / <a:sysClr> / <a:latin> to the right parent.
    let mut path: Vec<String> = Vec::new();
    let mut active_color_role: Option<String> = None;
    let mut active_font_role: Option<&'static str> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let local = local_name(&e);
                if local == "theme" {
                    theme_name = attr(&e, "name");
                }
                if is_color_role(&local) && in_scheme(&path, "clrScheme") {
                    active_color_role = Some(local.clone());
                }
                if (local == "majorFont" || local == "minorFont") && in_scheme(&path, "fontScheme")
                {
                    active_font_role = Some(if local == "majorFont" {
                        "major"
                    } else {
                        "minor"
                    });
                }
                path.push(local);
            }
            Ok(Event::Empty(e)) => {
                let local = local_name(&e);
                handle_color_or_font_leaf(
                    &local,
                    &e,
                    &active_color_role,
                    active_font_role,
                    &mut colors,
                    &mut major_font,
                    &mut minor_font,
                );
            }
            Ok(Event::End(_)) => {
                if let Some(closed) = path.pop() {
                    if Some(closed.as_str()) == active_color_role.as_deref() {
                        active_color_role = None;
                    }
                    if (closed == "majorFont" && active_font_role == Some("major"))
                        || (closed == "minorFont" && active_font_role == Some("minor"))
                    {
                        active_font_role = None;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => anyhow::bail!("XML error in theme1.xml: {e}"),
        }
        buf.clear();
    }

    Ok((theme_name, colors, major_font, minor_font))
}

fn handle_color_or_font_leaf(
    local: &str,
    e: &BytesStart,
    active_color_role: &Option<String>,
    active_font_role: Option<&'static str>,
    colors: &mut Vec<ThemeColor>,
    major_font: &mut Option<String>,
    minor_font: &mut Option<String>,
) {
    if let Some(role) = active_color_role {
        let hex = match local {
            "srgbClr" => attr(e, "val"),
            "sysClr" => attr(e, "lastClr").or_else(|| attr(e, "val")),
            _ => None,
        };
        if let Some(h) = hex {
            colors.push(ThemeColor {
                role: role.clone(),
                hex: h.to_uppercase(),
            });
        }
    }
    if local == "latin" {
        if let Some(face) = attr(e, "typeface") {
            match active_font_role {
                Some("major") => *major_font = Some(face),
                Some("minor") => *minor_font = Some(face),
                _ => {}
            }
        }
    }
}

fn in_scheme(path: &[String], scheme: &str) -> bool {
    path.iter().rev().any(|p| p == scheme)
}

fn is_color_role(local: &str) -> bool {
    matches!(
        local,
        "dk1"
            | "lt1"
            | "dk2"
            | "lt2"
            | "accent1"
            | "accent2"
            | "accent3"
            | "accent4"
            | "accent5"
            | "accent6"
            | "hlink"
            | "folHlink"
    )
}

// ── Slide parsing ─────────────────────────────────────────────────────────────

/// Read slide N: text frames + relationship-resolved image filenames.
fn read_slide<R: Read + std::io::Seek>(zip: &mut ZipArchive<R>, n: usize) -> Result<SlideContent> {
    let slide_xml = read_zip_string(zip, &format!("ppt/slides/slide{n}.xml"))?;
    let rels = read_zip_string(zip, &format!("ppt/slides/_rels/slide{n}.xml.rels")).ok();
    let images = rels.map(|r| extract_image_targets(&r)).unwrap_or_default();
    let texts = extract_slide_texts(&slide_xml);

    // First text frame is treated as the title; the rest as body. This is
    // best-effort — OOXML does mark title placeholders, but the heuristic is
    // good enough for charter inspection and avoids the placeholder dance.
    let mut iter = texts.into_iter();
    let title = iter.next().unwrap_or_default();
    let body = iter.collect::<Vec<_>>().join("\n");

    Ok(SlideContent {
        index: n,
        title,
        body,
        images,
    })
}

/// Walk the slide XML and collect each text frame as one joined string.
/// A "text frame" is one `<p:txBody>`; inside, `<a:p>` paragraphs are joined
/// with newlines and `<a:t>` runs are concatenated within each paragraph.
fn extract_slide_texts(xml: &str) -> Vec<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut frames: Vec<String> = Vec::new();
    let mut current_frame: Option<Vec<String>> = None; // paragraphs in the current <p:txBody>
    let mut current_paragraph: Option<String> = None;
    let mut in_text = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match local_name(&e).as_str() {
                "txBody" => current_frame = Some(Vec::new()),
                "p" if current_frame.is_some() => current_paragraph = Some(String::new()),
                "t" => in_text = true,
                _ => {}
            },
            Ok(Event::End(e)) => match local_name_end(&e).as_str() {
                "txBody" => {
                    if let Some(paras) = current_frame.take() {
                        let joined = paras.join("\n");
                        if !joined.trim().is_empty() {
                            frames.push(joined);
                        }
                    }
                }
                "p" => {
                    if let Some(para) = current_paragraph.take() {
                        if let Some(frame) = current_frame.as_mut() {
                            frame.push(para);
                        }
                    }
                }
                "t" => in_text = false,
                _ => {}
            },
            Ok(Event::Text(t)) if in_text => {
                if let Some(para) = current_paragraph.as_mut() {
                    if let Ok(s) = t.unescape() {
                        para.push_str(&s);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break, // best-effort: stop on first XML error
        }
        buf.clear();
    }

    frames
}

/// Pull image file basenames from a slide's `.rels` file.
fn extract_image_targets(rels_xml: &str) -> Vec<String> {
    let mut reader = Reader::from_str(rels_xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                if local_name(&e) == "Relationship" {
                    let ty = attr(&e, "Type").unwrap_or_default();
                    if ty.ends_with("/image") {
                        if let Some(target) = attr(&e, "Target") {
                            out.push(file_basename(&target));
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        buf.clear();
    }
    out
}

fn extract_layout_name(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut name: Option<String> = None;
    let mut user_name: Option<String> = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if local_name(&e) == "cSld" {
                    name = attr(&e, "name");
                }
                // Some templates set a more readable name in <p:sldLayout type="...">.
                if local_name(&e) == "sldLayout" {
                    user_name = attr(&e, "type");
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        buf.clear();
    }
    name.or(user_name)
}

// ── XML small helpers ─────────────────────────────────────────────────────────

fn local_name(e: &BytesStart) -> String {
    let raw = e.name();
    let bytes = raw.as_ref();
    let local = match bytes.iter().position(|b| *b == b':') {
        Some(i) => &bytes[i + 1..],
        None => bytes,
    };
    String::from_utf8_lossy(local).to_string()
}

fn local_name_end(e: &quick_xml::events::BytesEnd) -> String {
    let raw = e.name();
    let bytes = raw.as_ref();
    let local = match bytes.iter().position(|b| *b == b':') {
        Some(i) => &bytes[i + 1..],
        None => bytes,
    };
    String::from_utf8_lossy(local).to_string()
}

fn attr(e: &BytesStart, key: &str) -> Option<String> {
    for a in e.attributes().with_checks(false).flatten() {
        let raw = a.key.as_ref();
        let local = match raw.iter().position(|b| *b == b':') {
            Some(i) => &raw[i + 1..],
            None => raw,
        };
        if local == key.as_bytes() {
            return Some(String::from_utf8_lossy(&a.value).to_string());
        }
    }
    None
}

// ── Markdown rendering ────────────────────────────────────────────────────────

fn render_markdown(c: &BrandCharter) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Brand charter — {}\n\n",
        strip_pptx_ext(&c.source_filename)
    ));

    out.push_str("## Theme\n\n");
    out.push_str(&format!(
        "- **Theme name** : {}\n",
        c.theme_name.as_deref().unwrap_or("(unspecified)")
    ));
    out.push_str(&format!(
        "- **Heading font** : {}\n",
        c.major_font.as_deref().unwrap_or("(unspecified)")
    ));
    out.push_str(&format!(
        "- **Body font** : {}\n\n",
        c.minor_font.as_deref().unwrap_or("(unspecified)")
    ));

    out.push_str("## Colors\n\n");
    if c.colors.is_empty() {
        out.push_str("_(no theme colors detected — template may use a default Office theme)_\n\n");
    } else {
        out.push_str("| Role | Hex |\n|---|---|\n");
        for col in &c.colors {
            out.push_str(&format!("| {} | `#{}` |\n", col.role, col.hex));
        }
        out.push('\n');
    }

    out.push_str("## Slide layouts\n\n");
    if c.layouts.is_empty() {
        out.push_str("_(no layouts detected)_\n\n");
    } else {
        for l in &c.layouts {
            out.push_str(&format!("- {l}\n"));
        }
        out.push('\n');
    }

    out.push_str("## Assets extracted\n\n");
    if c.assets.is_empty() {
        out.push_str("_(no media)_\n\n");
    } else {
        for a in &c.assets {
            out.push_str(&format!(
                "- `{}` → `{}`\n",
                a.original_name,
                a.written_to.display()
            ));
        }
        out.push('\n');
    }

    out.push_str(&format!("## Slides ({})\n\n", c.slides.len()));
    for s in &c.slides {
        out.push_str(&format!("### Slide {} — {}\n\n", s.index, slide_heading(s)));
        if !s.body.trim().is_empty() {
            out.push_str(&s.body);
            out.push_str("\n\n");
        }
        if !s.images.is_empty() {
            out.push_str("**Images** : ");
            out.push_str(&s.images.join(", "));
            out.push_str("\n\n");
        }
    }
    out
}

fn slide_heading(s: &SlideContent) -> String {
    if s.title.trim().is_empty() {
        "(no title)".to_string()
    } else {
        s.title.replace('\n', " ").trim().to_string()
    }
}

fn strip_pptx_ext(name: &str) -> String {
    name.strip_suffix(".pptx")
        .or_else(|| name.strip_suffix(".PPTX"))
        .unwrap_or(name)
        .to_string()
}

// ── Default impl for theme tuple ──────────────────────────────────────────────

// `parse_theme().unwrap_or_default()` needs Default on the tuple — which it has
// because every component implements Default. Nothing to do here; comment kept
// for the next reader who wonders why the call site compiles.
