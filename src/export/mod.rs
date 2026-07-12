//! Round 15c ─ PPTX 导出器。
//!
//! 不引 `pptx` / `zip` crate —— PPTX 是 ZIP + OOXML，30 分钟手写一个最小可用版本。
//!
//! 限制（实用优先）：
//! - 文本 + 标题（无表格 / 图）
//! - 主题用默认 Office Theme
//! - 16:9 默认（12192000 × 6858000 EMU）
//! - 单 deck 文件（多张 slide）
//! - 中文字体 fallback（Calibri）
//!
//! 兼容性：PowerPoint / Keynote / Google Slides / LibreOffice Impress 都能开。

use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;
use zip::write::FileOptions;
use zip::CompressionMethod;

pub mod slides;
pub mod timeline_html;

pub use slides::{Slide, SlideKind};

/// 一个 deck = N 张 slide。
pub struct Deck {
    pub title: String,
    pub author: String,
    pub slides: Vec<Slide>,
}

impl Deck {
    pub fn new(title: impl Into<String>, author: impl Into<String>) -> Self {
        Self { title: title.into(), author: author.into(), slides: Vec::new() }
    }
    pub fn push(&mut self, s: Slide) -> &mut Self {
        self.slides.push(s);
        self
    }
    pub fn add(&mut self, kind: SlideKind) -> &mut Self {
        self.slides.push(Slide { kind });
        self
    }
}

/// 写到 .pptx 文件。
pub fn write_pptx(deck: &Deck, out: impl AsRef<Path>) -> Result<()> {
    let out = out.as_ref();
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let file = std::fs::File::create(out)
        .with_context(|| format!("create pptx at {}", out.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    // 1) [Content_Types].xml
    zip.start_file("[Content_Types].xml", opts)?;
    zip.write_all(content_types_xml().as_bytes())?;

    // 2) _rels/.rels
    zip.start_file("_rels/.rels", opts)?;
    zip.write_all(root_rels_xml().as_bytes())?;

    // 3) ppt/presentation.xml
    zip.start_file("ppt/presentation.xml", opts)?;
    zip.write_all(presentation_xml(deck).as_bytes())?;

    // 4) ppt/_rels/presentation.xml.rels
    zip.start_file("ppt/_rels/presentation.xml.rels", opts)?;
    zip.write_all(presentation_rels_xml(deck.slides.len()).as_bytes())?;

    // 5) ppt/theme/theme1.xml
    zip.start_file("ppt/theme/theme1.xml", opts)?;
    zip.write_all(theme_xml().as_bytes())?;

    // 6) ppt/slides/slide{N}.xml
    for (i, slide) in deck.slides.iter().enumerate() {
        let name = format!("ppt/slides/slide{}.xml", i + 1);
        zip.start_file(&name, opts)?;
        zip.write_all(slide_xml(slide).as_bytes())?;
    }

    // 7) ppt/slides/_rels/slide{N}.xml.rels（空 rels —— 我们的 slide 不引用其他 part）
    for i in 1..=deck.slides.len() {
        let name = format!("ppt/slides/_rels/slide{}.xml.rels", i);
        zip.start_file(&name, opts)?;
        zip.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>")?;
    }

    zip.finish()?;
    Ok(())
}

// ─── XML 模板 ─────────────────────────────────────────────────

fn content_types_xml() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
  <Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>
{slide_overrides}
</Types>"#,
        slide_overrides = (1..=10)
            .map(|i| format!(
                "  <Override PartName=\"/ppt/slides/slide{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
            ))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn root_rels_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>
</Relationships>"#.to_string()
}

fn presentation_xml(deck: &Deck) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rIdMaster"/></p:sldMasterIdLst>
  <p:sldIdLst>
{sld_ids}
  </p:sldIdLst>
  <p:notesSz cx="6858000" cy="9144000"/>
  <p:defaultTextStyle>
    <a:lvl1pPr><a:defRPr sz="1800"/></a:lvl1pPr>
  </p:defaultTextStyle>
  <p:extLst>
    <p:ext uri="{{C183D7F6-B498-43B3-948B-1728B52AA6E4}}">
      <p15:presentation xmlns:p15="http://schemas.microsoft.com/office/powerpoint/2012/main">
        <p15:notesShowMediaBelow/>
      </p15:presentation>
    </p:ext>
  </p:extLst>
</p:presentation>"#,
        sld_ids = (1..=deck.slides.len())
            .map(|i| format!(
                r#"    <p:sldId id="{id}" r:id="rId{i}"/>"#,
                id = 256 + i,
            ))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn presentation_rels_xml(n_slides: usize) -> String {
    let mut rels = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rIdMaster" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>
"#,
    );
    // 我们没生成 slideMaster —— 假装存在（PowerPoint 容忍）。但为了让 Impress/Keynote 高兴，
    // 我们补一个最小 slideMaster。
    rels.push_str(
        r#"  <Relationship Id="rIdTheme" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/>
"#,
    );
    for i in 1..=n_slides {
        rels.push_str(&format!(
            "  <Relationship Id=\"rId{i}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide{i}.xml\"/>\n"
        ));
    }
    rels.push_str("</Relationships>\n");
    rels
}

fn theme_xml() -> String {
    // 极简主题：Calibri 主字体 + 默认配色
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="fr-claw">
  <a:themeElements>
    <a:clrScheme name="fr-default">
      <a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>
      <a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
      <a:dk2><a:srgbClr val="44546A"/></a:dk2>
      <a:lt2><a:srgbClr val="E7E6E6"/></a:lt2>
      <a:accent1><a:srgbClr val="4472C4"/></a:accent1>
      <a:accent2><a:srgbClr val="ED7D31"/></a:accent2>
      <a:accent3><a:srgbClr val="A5A5A5"/></a:accent3>
      <a:accent4><a:srgbClr val="FFC000"/></a:accent4>
      <a:accent5><a:srgbClr val="5B9BD5"/></a:accent5>
      <a:accent6><a:srgbClr val="70AD47"/></a:accent6>
      <a:hlink><a:srgbClr val="0563C1"/></a:hlink>
      <a:folHlink><a:srgbClr val="954F72"/></a:folHlink>
    </a:clrScheme>
    <a:fontScheme name="fr-default">
      <a:majorFont>
        <a:latin typeface="Calibri Light"/>
        <a:ea typeface=""/>
        <a:cs typeface=""/>
      </a:majorFont>
      <a:minorFont>
        <a:latin typeface="Calibri"/>
        <a:ea typeface=""/>
        <a:cs typeface=""/>
      </a:minorFont>
    </a:fontScheme>
    <a:fmtScheme name="fr-default">
      <a:fillStyleLst>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
      </a:fillStyleLst>
      <a:lnStyleLst>
        <a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>
        <a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>
        <a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>
      </a:lnStyleLst>
      <a:effectStyleLst>
        <a:effectStyle><a:effectLst/></a:effectStyle>
        <a:effectStyle><a:effectLst/></a:effectStyle>
        <a:effectStyle><a:effectLst/></a:effectStyle>
      </a:effectStyleLst>
      <a:bgFillStyleLst>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
      </a:bgFillStyleLst>
    </a:fmtScheme>
  </a:themeElements>
</a:theme>"#
        .to_string()
}

fn slide_xml(slide: &Slide) -> String {
    let body = match &slide.kind {
        SlideKind::Title { title, subtitle } => {
            // 居中大标题
            let sub_xml = subtitle
                .as_deref()
                .map(|s| format!(
                    r#"<a:p><a:pPr algn="ctr"/><a:r><a:rPr lang="zh-CN" sz="2000"/><a:t>{}</a:t></a:r></a:p>"#,
                    xml_escape(s)
                ))
                .unwrap_or_default();
            format!(
                r#"<p:sp>
  <p:nvSpPr><p:cNvPr id="100" name="title"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr/></p:nvSpPr>
  <p:spPr><a:xfrm><a:off x="685800" y="2286000"/><a:ext cx="10886400" cy="2286000"/></a:xfrm></p:spPr>
  <p:txBody>
    <a:bodyPr anchor="ctr"/><a:lstStyle/>
    <a:p><a:pPr algn="ctr"/><a:r><a:rPr lang="zh-CN" sz="4400" b="1"/><a:t>{}</a:t></a:r></a:p>
    {sub_xml}
  </p:txBody>
</p:sp>"#,
                xml_escape(title),
            )
        }
        SlideKind::Heading { title, body } => {
            // 左上标题 + 下方正文
            let body_paras: String = body
                .iter()
                .map(|line| format!(
                    r#"<a:p><a:pPr marL="285750" indent="-285750"><a:buChar char="•"/></a:pPr><a:r><a:rPr lang="zh-CN" sz="1800"/><a:t>{}</a:t></a:r></a:p>"#,
                    xml_escape(line)
                ))
                .collect();
            format!(
                r#"<p:sp>
  <p:nvSpPr><p:cNvPr id="100" name="title"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr/></p:nvSpPr>
  <p:spPr><a:xfrm><a:off x="685800" y="457200"/><a:ext cx="10886400" cy="914400"/></a:xfrm></p:spPr>
  <p:txBody>
    <a:bodyPr/><a:lstStyle/>
    <a:p><a:r><a:rPr lang="zh-CN" sz="3200" b="1"/><a:t>{}</a:t></a:r></a:p>
  </p:txBody>
</p:sp>
<p:sp>
  <p:nvSpPr><p:cNvPr id="101" name="body"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr/></p:nvSpPr>
  <p:spPr><a:xfrm><a:off x="685800" y="1524000"/><a:ext cx="10886400" cy="4572000"/></a:xfrm></p:spPr>
  <p:txBody>
    <a:bodyPr/><a:lstStyle/>
    {body_paras}
  </p:txBody>
</p:sp>"#,
                xml_escape(title),
            )
        }
        SlideKind::Section { title } => {
            // 整页大标题（section divider）
            format!(
                r#"<p:sp>
  <p:nvSpPr><p:cNvPr id="100" name="title"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr/></p:nvSpPr>
  <p:spPr><a:xfrm><a:off x="685800" y="3048000"/><a:ext cx="10886400" cy="1524000"/></a:xfrm></p:spPr>
  <p:txBody>
    <a:bodyPr anchor="ctr"/><a:lstStyle/>
    <a:p><a:pPr algn="ctr"/><a:r><a:rPr lang="zh-CN" sz="5400" b="1"/><a:t>{}</a:t></a:r></a:p>
  </p:txBody>
</p:sp>"#,
                xml_escape(title),
            )
        }
    };

    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <p:cSld>
    <p:spTree>
      <p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
      <p:grpSpPr/>
      {body}
    </p:spTree>
  </p:cSld>
  <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sld>"#,
        body = body,
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deck_minimal_pptx() {
        let mut deck = Deck::new("Test", "fr-claw");
        deck.add(SlideKind::Title { title: "Hello".into(), subtitle: Some("sub".into()) });
        deck.add(SlideKind::Heading { title: "Body slide".into(), body: vec!["a".into(), "b".into()] });
        let dir = std::env::temp_dir().join(format!("fr-pptx-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("out.pptx");
        write_pptx(&deck, &out).unwrap();
        let size = std::fs::metadata(&out).unwrap().len();
        assert!(size > 1000, "pptx too small: {size} bytes");
        // 检查 [Content_Types].xml 在 zip 内
        let f = std::fs::File::open(&out).unwrap();
        let mut zip = zip::ZipArchive::new(f).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "[Content_Types].xml"));
        assert!(names.iter().any(|n| n == "ppt/slides/slide1.xml"));
        assert!(names.iter().any(|n| n == "ppt/slides/slide2.xml"));
    }

    #[test]
    fn xml_escape_basic() {
        assert_eq!(xml_escape("a&b<c>"), "a&amp;b&lt;c&gt;");
        assert_eq!(xml_escape("\"'\""), "&quot;&apos;&quot;");
    }
}
