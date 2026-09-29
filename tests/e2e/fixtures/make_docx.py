"""A fabricated speech-therapist summary as a real .docx (header, footer, author metadata)."""
import sys
import zipfile

W = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"'


def p(text, bold=False):
    rpr = "<w:rPr><w:b/><w:rtl/></w:rPr>" if bold else "<w:rPr><w:rtl/></w:rPr>"
    return f'<w:p><w:pPr><w:bidi/></w:pPr><w:r>{rpr}<w:t xml:space="preserve">{text}</w:t></w:r></w:p>'


def row(a, b):
    cell = lambda t: f"<w:tc><w:tcPr><w:tcW w:w=\"2400\" w:type=\"dxa\"/></w:tcPr>{p(t)}</w:tc>"
    return f"<w:tr>{cell(a)}{cell(b)}</w:tr>"


body = "".join([
    p("סיכום אבחון שפה ותקשורת", bold=True),
    p("נועם, בן 5 ו-4 חודשים, הופנה לאבחון בשל קושי בהבנת הוראות מורכבות ובסיפור רצף אירועים."),
    p("במבחן גולדמן התקבל ציון תקן 85, בטווח הממוצע הנמוך. אוצר המילים ההבעתי תקין לגיל."),
    p("במהלך האבחון שיתף פעולה ונעזר בתמונות כדי לספר. דנה, אמו, סיפרה שבבית הוא מדבר בחופשיות ובהנאה."),
    '<w:tbl><w:tblPr><w:bidiVisual/><w:tblW w:w="4800" w:type="dxa"/></w:tblPr>'
    + row("מבחן", "ציון") + row("הבנה", "85") + row("הבעה", "92") + "</w:tbl>",
    p("המלצות: טיפול בתקשורת פעמיים בשבוע, הדרכת הורים ומעקב בעוד חצי שנה."),
    '<w:sectPr><w:headerReference w:type="default" r:id="rIdH"/><w:footerReference w:type="default" r:id="rIdF"/><w:bidi/></w:sectPr>',
])
files = {
    "[Content_Types].xml": '<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>',
    "_rels/.rels": '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>',
    "word/_rels/document.xml.rels": '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdH" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rIdF" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/></Relationships>',
    "word/document.xml": f'<?xml version="1.0" encoding="UTF-8"?><w:document {W}><w:body>{body}</w:body></w:document>',
    "word/header1.xml": f'<?xml version="1.0" encoding="UTF-8"?><w:hdr {W}>{p("מכון בדוי לתקשורת · קלינאית התקשורת: יעל בדויה")}</w:hdr>',
    "word/footer1.xml": f'<?xml version="1.0" encoding="UTF-8"?><w:ftr {W}>{p("רחוב הבדיה 1, עיר בדויה · עמוד 1")}</w:ftr>',
    "docProps/core.xml": '<?xml version="1.0" encoding="UTF-8"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:creator>יעל בדויה</dc:creator><dc:title>סיכום אבחון</dc:title></cp:coreProperties>',
}
with zipfile.ZipFile(sys.argv[1], "w", zipfile.ZIP_DEFLATED) as z:
    for name, data in files.items():
        z.writestr(name, data)
