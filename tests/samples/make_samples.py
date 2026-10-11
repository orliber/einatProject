"""Sample documents for trying the app: one fabricated child, two assessment rounds.

Everything here is invented. No person, kindergarten or clinic is real. The files cover every
format the app imports (Word, ODT, PDF, text) and the traps it must catch: names in headers,
footers and file properties, hidden comments, tracked changes, a name the case does not list,
a fake ID, phone and e-mail, and a scanned PDF it must refuse.

    python3 tests/samples/make_samples.py [output folder]     (default: ./sample-documents)

PDFs are made with LibreOffice when it is installed (`soffice`); without it they are skipped.
The phone, e-mail and ID are assembled at run time, so this file passes `cargo xtask scan`.
"""
import os
import shutil
import subprocess
import sys
import tempfile
import zipfile
import zlib

OUT = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "sample-documents")
FOLLOW = os.path.join(OUT, "מעקב - שנה אחרי")
TAMAR_DIR = os.path.join(OUT, "תמר")

# The fabricated family. A known-fake ID (valid check digit, allowed by the repo scan).
CHILD, MOTHER, FATHER, SISTER = "אלון", "שירה", "גיא", "נוגה"
SURNAME = "בדיוני"
TEACHER, GAN, TOWN = "אורית", "גן השקד", "גבעת הרימון"
SLP, OT, DOCTOR = "ליאת", "דפנה", 'ד"ר יואב רון'
STRANGER = "עומרי"  # a friend the case does not list: must be raised as a suspect
FAKE_ID = "0000000" + "26"
PHONE = "05" + "0-" + "000" + "-" + "0000"
MAIL = "shira.fake" + "@" + "example.com"

# The second case, a toddler (the demo script's case). Same rules: nobody here is real.
TAMAR, T_SURNAME, T_MOTHER, T_FATHER, T_BROTHER = "תמר", "דמיוני", "מיכל", "עידו", "איתי"
T_CARER, T_NURSERY, T_TOWN = "רינה", "מעון הרקפת", "קריית הדמיון"
T_SLP, T_DOCTOR = "הדס", 'ד"ר נעמה אלמוגי'
T_FRIEND, T_GRANNY = "עלמה", "זהבה"  # not listed in the case: must be hidden on their own
T_PHONE = "05" + "4-" + "000" + "-" + "0000"
T_MAIL = "michal.fake" + "@" + "example.com"

# --- Word -------------------------------------------------------------------------------

W = ('xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" '
     'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"')


def wp(text, bold=False):
    rpr = "<w:rPr><w:b/><w:rtl/></w:rPr>" if bold else "<w:rPr><w:rtl/></w:rPr>"
    return (f'<w:p><w:pPr><w:bidi/></w:pPr><w:r>{rpr}'
            f'<w:t xml:space="preserve">{esc(text)}</w:t></w:r></w:p>')


def wtable(rows):
    def cell(t):
        return f'<w:tc><w:tcPr><w:tcW w:w="2400" w:type="dxa"/></w:tcPr>{wp(t)}</w:tc>'
    body = "".join("<w:tr>" + "".join(cell(c) for c in r) + "</w:tr>" for r in rows)
    return f'<w:tbl><w:tblPr><w:bidiVisual/><w:tblW w:w="4800" w:type="dxa"/></w:tblPr>{body}</w:tbl>'


def esc(t):
    return t.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def docx(path, parts, header, footer, creator, title):
    """A real .docx: body, header, footer and author properties."""
    body = "".join(parts) + (
        '<w:sectPr><w:headerReference w:type="default" r:id="rIdH"/>'
        '<w:footerReference w:type="default" r:id="rIdF"/><w:bidi/></w:sectPr>')
    ct = "application/vnd.openxmlformats-officedocument.wordprocessingml"
    files = {
        "[Content_Types].xml":
            '<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
            '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
            '<Default Extension="xml" ContentType="application/xml"/>'
            f'<Override PartName="/word/document.xml" ContentType="{ct}.document.main+xml"/>'
            f'<Override PartName="/word/header1.xml" ContentType="{ct}.header+xml"/>'
            f'<Override PartName="/word/footer1.xml" ContentType="{ct}.footer+xml"/>'
            '<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>',
        "_rels/.rels":
            '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>'
            '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>',
        "word/_rels/document.xml.rels":
            '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            '<Relationship Id="rIdH" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>'
            '<Relationship Id="rIdF" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/></Relationships>',
        "word/document.xml": f'<?xml version="1.0" encoding="UTF-8"?><w:document {W}><w:body>{body}</w:body></w:document>',
        "word/header1.xml": f'<?xml version="1.0" encoding="UTF-8"?><w:hdr {W}>{wp(header)}</w:hdr>',
        "word/footer1.xml": f'<?xml version="1.0" encoding="UTF-8"?><w:ftr {W}>{wp(footer)}</w:ftr>',
        "docProps/core.xml":
            '<?xml version="1.0" encoding="UTF-8"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" '
            f'xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:creator>{esc(creator)}</dc:creator><dc:title>{esc(title)}</dc:title></cp:coreProperties>',
    }
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in files.items():
            z.writestr(name, data)


# --- ODT --------------------------------------------------------------------------------

NS = ('xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
      'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" '
      'xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" '
      'xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" '
      'xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" '
      'xmlns:dc="http://purl.org/dc/elements/1.1/" '
      'xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" '
      'xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"')


def op(text, extra=""):
    return f'<text:p text:style-name="R">{esc(text)}{extra}</text:p>'


def odt(path, parts, header, footer, creator, title):
    """A real .odt (RTL paragraphs): body, header, footer and author properties."""
    content = (f'<?xml version="1.0" encoding="UTF-8"?><office:document-content {NS} office:version="1.3">'
               '<office:automatic-styles><style:style style:name="R" style:family="paragraph">'
               '<style:paragraph-properties fo:text-align="end" style:writing-mode="rl-tb"/></style:style>'
               '</office:automatic-styles><office:body><office:text>'
               + "".join(parts) + '</office:text></office:body></office:document-content>')
    styles = (f'<?xml version="1.0" encoding="UTF-8"?><office:document-styles {NS} office:version="1.3">'
              '<office:automatic-styles><style:page-layout style:name="pm1"><style:page-layout-properties '
              'fo:page-width="21cm" fo:page-height="29.7cm" style:writing-mode="rl-tb"/></style:page-layout>'
              '</office:automatic-styles><office:master-styles>'
              '<style:master-page style:name="Standard" style:page-layout-name="pm1">'
              f'<style:header><text:p>{esc(header)}</text:p></style:header>'
              f'<style:footer><text:p>{esc(footer)}</text:p></style:footer>'
              '</style:master-page></office:master-styles></office:document-styles>')
    meta = (f'<?xml version="1.0" encoding="UTF-8"?><office:document-meta {NS} office:version="1.3"><office:meta>'
            f'<meta:initial-creator>{esc(creator)}</meta:initial-creator><dc:creator>{esc(creator)}</dc:creator>'
            f'<dc:title>{esc(title)}</dc:title></office:meta></office:document-meta>')
    manifest = (f'<?xml version="1.0" encoding="UTF-8"?><manifest:manifest {NS} manifest:version="1.3">'
                '<manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.text"/>'
                '<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>'
                '<manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/>'
                '<manifest:file-entry manifest:full-path="meta.xml" manifest:media-type="text/xml"/>'
                '</manifest:manifest>')
    with zipfile.ZipFile(path, "w") as z:
        z.writestr(zipfile.ZipInfo("mimetype"), "application/vnd.oasis.opendocument.text",
                   compress_type=zipfile.ZIP_STORED)
        for name, data in [("content.xml", content), ("styles.xml", styles), ("meta.xml", meta),
                           ("META-INF/manifest.xml", manifest)]:
            z.writestr(name, data, compress_type=zipfile.ZIP_DEFLATED)


def to_pdf(odt_path, pdf_path):
    """ODT to PDF with LibreOffice; False when it is not installed."""
    soffice = shutil.which("soffice") or shutil.which("libreoffice")
    if not soffice:
        return False
    with tempfile.TemporaryDirectory() as tmp:
        subprocess.run([soffice, f"-env:UserInstallation=file://{tmp}/profile", "--headless",
                        "--convert-to", "pdf", "--outdir", tmp, odt_path],
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        made = os.path.join(tmp, os.path.splitext(os.path.basename(odt_path))[0] + ".pdf")
        # soffice can exit 0 without writing anything (a broken or sandboxed install):
        # treat that like LibreOffice missing rather than failing the whole run.
        if not os.path.exists(made):
            return False
        shutil.move(made, pdf_path)
    return True


def scanned_pdf(path):
    """A page that is only a picture (like a scanner makes): the app must refuse it."""
    w, h = 120, 160
    rows = bytearray()
    for y in range(h):
        for x in range(w):
            rows.append(255 if (y // 8) % 3 else (90 if x % 10 < 7 else 255))
    img = zlib.compress(bytes(rows))
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /XObject << /Im1 4 0 R >> >> /Contents 5 0 R >>",
        b"<< /Type /XObject /Subtype /Image /Width %d /Height %d /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length %d >>\nstream\n"
        % (w, h, len(img)) + img + b"\nendstream",
    ]
    draw = b"q 480 0 0 640 57 100 cm /Im1 Do Q"
    objs.append(b"<< /Length %d >>\nstream\n" % len(draw) + draw + b"\nendstream")
    out = bytearray(b"%PDF-1.4\n")
    offsets = []
    for i, o in enumerate(objs, 1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % i + o + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
    for off in offsets:
        out += b"%010d 00000 n \n" % off
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, xref)
    with open(path, "wb") as f:
        f.write(out)


def text(path, body):
    with open(path, "w", encoding="utf-8") as f:
        f.write(body.strip() + "\n")


# --- The documents ----------------------------------------------------------------------

def first_round(made):
    docx(os.path.join(OUT, "01 שאלון קליטה להורים.docx"), [
        wp("שאלון קליטה לאבחון פסיכולוגי התפתחותי", bold=True),
        wtable([("שם הילד", f"{CHILD} {SURNAME}"), ("ת.ז.", FAKE_ID), ("גיל", "5 שנים ו-8 חודשים"),
                ("הורים", f"{MOTHER} ו{FATHER} {SURNAME}"), ("טלפון", PHONE), ("דוא\"ל", MAIL),
                ("כתובת", f"רחוב הזית 4, {TOWN}"), ("מסגרת", f"{GAN}, הגננת {TEACHER}")]),
        wp("סיבת הפנייה", bold=True),
        wp(f"לקראת המעבר לכיתה א' אנחנו מתלבטים אם {CHILD} מוכן. הגננת {TEACHER} אמרה שקשה לו לשבת "
           "במפגש ושהוא נעלב מהר כשמפסיד במשחק. בבית הוא ילד מתוק, סקרן ומלא דמיון."),
        wp("התפתחות מוקדמת", bold=True),
        wp("הריון ולידה תקינים. הלך בגיל 13 חודשים. מילים ראשונות בגיל שנה, משפטים בגיל שנתיים וחצי. "
           "בגיל 3 היה טיפול קצר בקלינאית תקשורת בגלל הגייה."),
        wp("משפחה", bold=True),
        wp(f"{CHILD} גר עם שני ההורים ועם אחותו {SISTER}, בת 4. {MOTHER} עובדת כמורה ו{FATHER} מהנדס. "
           "לפני שנה עברנו דירה, ו" + CHILD + " התקשה בהתחלה להסתגל לגן החדש."),
        wp("שינה ואכילה", bold=True),
        wp("נרדם בקושי, צריך שמישהו יישב לידו. אוכל מגוון מצומצם, בעיקר פסטה ואורז."),
        wp("מה אתם מקווים לקבל מהאבחון?", bold=True),
        wp("להבין מה עוזר לו, ואם כדאי שיישאר עוד שנה בגן."),
    ], header=f"שאלון קליטה · {CHILD} {SURNAME}", footer=f"מולא על ידי {MOTHER} {SURNAME} · {PHONE}",
        creator=f"{MOTHER} {SURNAME}", title=f"שאלון {CHILD}")
    made.append("01 שאלון קליטה להורים.docx")

    odt(os.path.join(OUT, "02 דיווח הגננת.odt"), [
        '<text:h text:outline-level="1">דיווח גננת לקראת אבחון</text:h>',
        op(f"{CHILD} נמצא בגן השנה השנייה. הוא ילד חם, שמח לעזור ואוהב במיוחד את פינת הבנייה."),
        op("במפגש הבוקר מתקשה לשבת יותר מכמה דקות, קם, נוגע בחברים ומדבר בלי לחכות לתורו.",
           '<office:annotation><dc:creator>אורית</dc:creator><text:p>הערה פנימית: לא לשלוח להורים</text:p></office:annotation>'),
        op(f"בחצר משחק בעיקר עם {STRANGER}. כשמפסיד במשחק או כשמשהו לא מצליח לו, בוכה או עוזב בכעס, "
           "ונרגע אחרי כמה דקות כשמתקרבים אליו בשקט."),
        op("במשימות גרפומוטוריות נמנע: אחיזת עיפרון לא בשלה, מעדיף לבנות ולא לצייר."),
        '<text:tracked-changes><text:changed-region text:id="c1"><text:deletion>'
        '<text:p>נוסח קודם: ייתכן שיש לו הפרעת קשב</text:p></text:deletion></text:changed-region></text:tracked-changes>',
        op("מה עוזר: הכנה מראש למעברים, תפקיד קבוע במפגש (למשל לחלק דפים), ומשוב חיובי מיידי."),
        op(f"נשמח לשיתוף פעולה. {TEACHER}, {GAN}"),
    ], header=f"{GAN} · {TOWN}", footer=f"הגננת {TEACHER} · {PHONE}", creator=TEACHER,
        title=f"דיווח על {CHILD}")
    made.append("02 דיווח הגננת.odt")

    docx(os.path.join(OUT, "03 סיכום קלינאית תקשורת.docx"), [
        wp("סיכום אבחון שפה ותקשורת", bold=True),
        wp(f"{CHILD}, בן 5 ו-6 חודשים, הופנה בשל קושי בסיפור רצף אירועים ובהבנת הוראות מורכבות."),
        wtable([("תחום", "ציון תקן"), ("הבנת שפה", "88"), ("הבעה", "95"), ("שיום", "102")]),
        wp("ההבנה בטווח הממוצע הנמוך; ההבעה ואוצר המילים תקינים לגיל. בסיפור מתמונות נעזר בשאלות מכוונות."),
        wp(f"לדברי {MOTHER}, בבית הוא מדבר הרבה ובהנאה, ולפעמים \"מדלג\" על חלקים בסיפור."),
        wp("המלצות: טיפול קבוצתי בתקשורת פעם בשבוע, והדרכת הורים לקריאה משותפת."),
    ], header=f"מכון בדוי לתקשורת · קלינאית התקשורת {SLP} {SURNAME}ת",
        footer=f"רחוב הבדיה 1, {TOWN} · עמוד 1", creator=f"{SLP} {SURNAME}ת", title="סיכום אבחון")
    made.append("03 סיכום קלינאית תקשורת.docx")

    # A folder of its own: two runs at once (parallel tests) must not share the ODT.
    tmp = tempfile.mkdtemp()
    ot_odt = os.path.join(tmp, "ot-report.odt")
    odt(ot_odt, [
        '<text:h text:outline-level="1">סיכום הערכה בריפוי בעיסוק</text:h>',
        op(f"{CHILD} הופנה על ידי הגננת בשל הימנעות מציור וכתיבה."),
        op("אחיזת עיפרון: אגרופית-מעברית. לחץ חזק על הדף, עייפות מהירה של כף היד."),
        op("תכנון תנועה: מתקשה בחיקוי תנוחות רצופות. שיווי משקל תקין לגיל."),
        op("ויסות חושי: מחפש תנועה (קופץ, מסתובב), רגיש לרעש חזק בחדר ההתעמלות."),
        op("המלצות: טיפול בריפוי בעיסוק פעם בשבוע למשך חצי שנה, ישיבה על כרית אוויר במפגש, "
           "והפסקות תנועה קצרות במשך היום."),
    ], header=f"מכון בדוי לריפוי בעיסוק · מרפאה בעיסוק {OT} {SURNAME}ת",
        footer=f"{CHILD} {SURNAME} · ת.ז. {FAKE_ID}", creator=OT, title="הערכה")
    if to_pdf(ot_odt, os.path.join(OUT, "04 סיכום ריפוי בעיסוק.pdf")):
        made.append("04 סיכום ריפוי בעיסוק.pdf")
    shutil.rmtree(tmp)

    text(os.path.join(OUT, "05 מכתב הפניה מרופא הילדים.txt"), f"""
לכבוד הפסיכולוגית ההתפתחותית,

הנדון: {CHILD} {SURNAME}, ת.ז. {FAKE_ID}

הילד מוכר לי מלידה. התפתחות גופנית תקינה, שמיעה וראייה נבדקו לאחרונה ונמצאו תקינות.
ההורים מדווחים על קושי בוויסות ובקשב בגן, לקראת כיתה א'.
אבקש הערכה פסיכולוגית התפתחותית והמלצה לגבי מוכנות לבית הספר.

בברכה,
{DOCTOR}, רופא ילדים, מרפאת {TOWN}
""")
    made.append("05 מכתב הפניה מרופא הילדים.txt")

    text(os.path.join(OUT, "06 הערות תצפית של עינת.txt"), f"""
תצפית בגן, בוקר.
{CHILD} הגיע ראשון לפינת הבנייה ובנה מגדל גבוה בריכוז רב, כעשר דקות.
במעבר למפגש המשיך לבנות. אחרי תזכורת אחת מהגננת הצטרף, אבל ישב בקצה ושיחק בשרוכים.
כשהגננת שאלה שאלה על הסיפור, ענה נכון ובהתלהבות, בלי להצביע.
בחצר שיחק עם {STRANGER} במשחק תופסת. כשנתפס, צעק "לא הוגן" ועזב. אחרי שלוש דקות חזר בעצמו.

פגישה אישית (שתי פגישות):
משתף פעולה, נהנה מהקשר ומהצלחות. במטלות מילוליות ארוכות מאבד עניין ומבקש "עוד כמה?".
במשימות חזותיות מתמיד ועובד בשיטתיות. ציור אדם: דמות פשוטה, אחיזה לא בשלה.
""")
    made.append("06 הערות תצפית של עינת.txt")

    text(os.path.join(OUT, "07 ציונים להקלדה (WPPSI-IV).txt"), """
ציונים בדויים להקלדה ידנית במסך "ציונים", מבחן WPPSI-IV:

מדדים (ציוני תקן):
  מנת משכל כללית FSIQ   98
  הבנה מילולית VCI       94
  חזותי-מרחבי VSI        112
  חשיבה פלואידית FRI     104
  זיכרון עבודה WMI       86
  מהירות עיבוד PSI       82

תת-מבחנים (ציונים מותאמים):
  מידע 9 · שיתופיות 9 · קוביות 13 · הרכבת עצמים 12 · מטריצות 11 · מושגים בתמונות 10
  זיכרון תמונות 7 · מיקומים בגן החיות 8 · חיפוש חרקים 6 · מחיקה 8
""")
    made.append("07 ציונים להקלדה (WPPSI-IV).txt")

    scanned_pdf(os.path.join(OUT, "08 מסמך סרוק - התוכנה צריכה לסרב.pdf"))
    made.append("08 מסמך סרוק - התוכנה צריכה לסרב.pdf")


def follow_up(made):
    docx(os.path.join(FOLLOW, "01 עדכון מהמורה בכיתה א.docx"), [
        wp("עדכון מחנכת כיתה א'", bold=True),
        wp(f"{CHILD} השתלב יפה בכיתה. בזכות הישיבה ליד השולחן הקדמי והפסקות התנועה הוא מצליח לעבוד "
           "ברוב השיעור."),
        wp("בקריאה מתקדם בקצב הכיתה. בכתיבה עדיין איטי, והמחברת לא מסודרת."),
        wp("חברתית: יש לו שני חברים קבועים. כשמפסיד עדיין נעלב, אבל נרגע מהר יותר ומבקש לשחק שוב."),
    ], header="בית ספר בדוי · כיתה א'2", footer="המחנכת טל", creator="טל", title="עדכון")
    made.append("מעקב - שנה אחרי/01 עדכון מהמורה בכיתה א.docx")

    text(os.path.join(FOLLOW, "02 ציונים להקלדה (WISC-V).txt"), """
אבחון מעקב, שנה וחודש אחרי. ציונים בדויים להקלדה ידנית, מבחן WISC-V:

מדדים (ציוני תקן):
  מנת משכל כללית FSIQ   101
  הבנה מילולית VCI       98
  חזותי-מרחבי VSI        114
  חשיבה פלואידית FRI     103
  זיכרון עבודה WMI       92
  מהירות עיבוד PSI       85

תת-מבחנים (ציונים מותאמים):
  שיתופיות 10 · אוצר מילים 9 · קוביות 13 · פאזלים חזותיים 12 · מטריצות 11 · משקלות 10
  זכירת ספרות 9 · זכירת תמונות 8 · צפנים 7 · חיפוש סמלים 8

להשוואה עם האבחון הראשון: המדדים המשותפים (FSIQ, VCI, VSI, FRI, WMI, PSI).
""")
    made.append("מעקב - שנה אחרי/02 ציונים להקלדה (WISC-V).txt")

    text(os.path.join(FOLLOW, "03 שיחה עם ההורים.txt"), f"""
שיחת מעקב עם {MOTHER} ו{FATHER}.
ההורים מספרים ש{CHILD} אוהב את בית הספר ומחכה לחוגי הבנייה.
הטיפול בריפוי בעיסוק הסתיים לפני חודשיים; האחיזה השתפרה.
עדיין קשה לו להירדם, אבל פחות. {SISTER} התחילה גן חובה והוא "עוזר לה" בבוקר.
""")
    made.append("מעקב - שנה אחרי/03 שיחה עם ההורים.txt")


def tamar(made):
    """The demo script's case: a toddler, every format, the traps of the first case."""
    os.makedirs(TAMAR_DIR, exist_ok=True)
    full = f"{TAMAR} {T_SURNAME}"
    docx(os.path.join(TAMAR_DIR, "01 טופס פנייה מההורים.docx"), [
        wp("טופס פנייה לאבחון התפתחותי", bold=True),
        wtable([("שם הילדה", full), ("ת.ז.", FAKE_ID), ("גיל", "3 שנים ו-3 חודשים"),
                ("הורים", f"{T_MOTHER} ו{T_FATHER} {T_SURNAME}"), ("טלפון", T_PHONE),
                ("דוא\"ל", T_MAIL), ("כתובת", f"שדרות הבדיה 12, {T_TOWN}"),
                ("מסגרת", f"{T_NURSERY}, המטפלת {T_CARER}")]),
        wp("סיבת הפנייה", bold=True),
        wp(f"{TAMAR} מדברת מעט מאוד ביחס לילדים בגילה, בעיקר מילים בודדות. במעון אומרים שהיא "
           "משחקת הרבה לבד. רופאת ההתפתחות המליצה על אבחון כדי להבין אם מדובר בעיכוב שפתי בלבד."),
        wp("התפתחות מוקדמת", bold=True),
        wp("לידה בשבוע 39, ללא סיבוכים. ישבה בגיל 7 חודשים, הלכה בגיל 14 חודשים. מילה ראשונה "
           "(\"אבא\") בגיל 18 חודשים. כיום כ-30 מילים, צירופי שתי מילים מעטים."),
        wp("משפחה", bold=True),
        wp(f"{TAMAR} גרה עם שני ההורים ועם אחיה {T_BROTHER}, בן 6. {T_MOTHER} אחות במקצועה "
           f"ו{T_FATHER} עובד בהייטק. בבית מדברים עברית בלבד."),
        wp("שינה ואכילה", bold=True),
        wp("ישנה לילה שלם. אוכלת מגוון, אבל לא אוהבת מרקמים חדשים."),
        wp("מה אתם מקווים לקבל מהאבחון?", bold=True),
        wp("להבין מה עוזר לה לדבר יותר, ואם צריך מסגרת או טיפול מיוחדים."),
    ], header=f"טופס פנייה · {full}", footer=f"מולא על ידי {T_MOTHER} {T_SURNAME} · {T_PHONE}",
        creator=f"{T_MOTHER} {T_SURNAME}", title=f"פנייה {TAMAR}")
    made.append("תמר/01 טופס פנייה מההורים.docx")

    odt(os.path.join(TAMAR_DIR, "02 דיווח המטפלת במעון.odt"), [
        '<text:h text:outline-level="1">דיווח מהמעון לקראת אבחון</text:h>',
        op(f"{TAMAR} נמצאת במעון שנה וחצי. היא מגיעה בשמחה, נפרדת מההורים בקלות ונהנית מפינת המים."),
        op("מבינה הוראות פשוטות בשגרה (\"לשטוף ידיים\", \"להביא נעליים\"), ומתקשה בהוראות חדשות.",
           f'<office:annotation><dc:creator>{T_CARER}</dc:creator>'
           '<text:p>הערה למנהלת: לא להעביר להורים</text:p></office:annotation>'),
        op(f"בחצר משחקת בעיקר לצד ילדים ולא איתם. עם {T_FRIEND} יש לה משחק קבוע של מסירת כדור, "
           "והיא מחייכת ומביטה בה כשהכדור מגיע."),
        op("כשהיא רוצה משהו, מושכת את המבוגר ביד או מצביעה. כשלא מבינים אותה, בוכה ומתיישבת על הרצפה."),
        '<text:tracked-changes><text:changed-region text:id="c1"><text:deletion>'
        '<text:p>נוסח קודם: אולי היא על הספקטרום</text:p></text:deletion></text:changed-region></text:tracked-changes>',
        op("מה עוזר: תמונות של סדר היום, שירים עם תנועות ידיים, ולתת לה זמן לענות."),
        op(f"בברכה, {T_CARER}, {T_NURSERY}"),
    ], header=f"{T_NURSERY} · {T_TOWN}", footer=f"המטפלת {T_CARER} · {T_PHONE}", creator=T_CARER,
        title=f"דיווח על {TAMAR}")
    made.append("תמר/02 דיווח המטפלת במעון.odt")

    docx(os.path.join(TAMAR_DIR, "03 מכתב מרופאת ההתפתחות.docx"), [
        wp("מכתב הפניה", bold=True),
        wp(f"הנדון: {full}, בת 3 ו-2 חודשים"),
        wp("הילדה נבדקה במכון להתפתחות הילד. בדיקת שמיעה (BERA) תקינה. בדיקה נוירולוגית תקינה. "
           "מוטוריקה גסה תקינה לגיל."),
        wp("עיקר הקושי: עיכוב בהתפתחות השפה המבטאת, והבנה בטווח הנמוך. קשר עין טוב עם מבוגרים מוכרים."),
        wp("אבקש הערכה פסיכולוגית התפתחותית כוללת, כולל התייחסות לתקשורת החברתית, והמלצות להמשך."),
        wp(f"בברכה, {T_DOCTOR}, רופאת התפתחות הילד"),
    ], header=f"מכון בדוי להתפתחות הילד · {T_TOWN}", footer=f"{T_DOCTOR} · מספר רישיון 00000",
        creator=T_DOCTOR, title="מכתב הפניה")
    made.append("תמר/03 מכתב מרופאת ההתפתחות.docx")

    tmp = tempfile.mkdtemp()
    slp_odt = os.path.join(tmp, "slp-report.odt")
    odt(slp_odt, [
        '<text:h text:outline-level="1">סיכום הערכת שפה ותקשורת</text:h>',
        op(f"{TAMAR} הופנתה בשל אוצר מילים מצומצם ומיעוט צירופים."),
        op("הבנת שפה: מזהה חפצים ותמונות מוכרים, מבצעת הוראה בת שלב אחד. מתקשה בהוראה בת שני שלבים."),
        op("הבעה: כשלושים מילים, בעיקר שמות עצם. משתמשת בהצבעה ובמחוות כדי להשלים."),
        op("תקשורת: יוזמת קשר עם המבוגר, משתפת בהנאה, אומרת \"תראי!\" בהצבעה ובמבט. "
           "משחק סמלי ראשוני: מאכילה בובה."),
        op("המלצות: טיפול בקלינאית תקשורת פעמיים בשבוע, והדרכת הורים לשפה בשגרה."),
    ], header=f"קלינאית התקשורת {T_SLP} · מכון בדוי", footer=f"{full} · ת.ז. {FAKE_ID}",
        creator=T_SLP, title="סיכום הערכת שפה ותקשורת")
    if to_pdf(slp_odt, os.path.join(TAMAR_DIR, "04 סיכום קלינאית התקשורת.pdf")):
        made.append("תמר/04 סיכום קלינאית התקשורת.pdf")
    shutil.rmtree(tmp)

    text(os.path.join(TAMAR_DIR, "05 מייל מההורים אחרי הפגישה.txt"), f"""
שלום,

תודה על הפגישה. רצינו להוסיף כמה דברים ששכחנו לספר.
בשבוע שעבר {TAMAR} אמרה לראשונה משפט של שלוש מילים: "אבא בוא שם".
בימי שלישי היא אצל סבתא {T_GRANNY}, ושם היא מדברת יותר, אולי כי זה שקט.
{T_BROTHER} מאוד עוזר לה ומשחק איתה במשחקי דמיון, היא מחקה אותו.

תודה,
{T_MOTHER}
{T_PHONE}
""")
    made.append("תמר/05 מייל מההורים אחרי הפגישה.txt")

    text(os.path.join(TAMAR_DIR, "06 הערות תצפית והערכה אישית.txt"), f"""
תצפית במעון, בוקר.
{TAMAR} שיחקה בפינת המים כרבע שעה, מילאה ושפכה כוסות בריכוז.
במפגש ישבה ליד {T_CARER}, הצטרפה לתנועות הידיים בשיר אבל לא שרה.
כשילד לקח לה כוס, צעקה "לא!" והסתכלה על {T_CARER} כאילו ביקשה עזרה.
בחצר שיחקה עם {T_FRIEND} במסירת כדור, חייכה, הביטה בה ובי לסירוגין.

פגישה אישית (שתי פגישות):
נפרדה מאמא אחרי כמה דקות. שיתפה פעולה בהנאה במשימות של פאזלים וקוביות.
במשימות שפה הצביעה נכון על רוב התמונות המוכרות, אבל לא ענתה במילים.
במשחק עם בובה: האכילה, השכיבה לישון וכיסתה. הביאה לי את הבובה כדי שאראה.
כשלא הצליחה להרכיב צורה, נתנה לי אותה ואמרה "עזרה".
""")
    made.append("תמר/06 הערות תצפית והערכה אישית.txt")

    text(os.path.join(TAMAR_DIR, "07 ציונים להקלדה.txt"), """
ציונים בדויים להקלדה ידנית במסך "ציונים". גיל במבחן: 3 שנים ו-3 חודשים.

Bayley-4 (ציוני תקן):
  קוגניטיבי 95 · שפה 74 · מוטורי 98
  תת-מבחנים (ציונים מותאמים): שפה קולטת 6 · שפה מבטאת 4 · מוטוריקה עדינה 9 · מוטוריקה גסה 10

ABAS-3, דיווח הורים:
  ציון מסתגל כללי 86 · מושגי 78 · חברתי 90 · מעשי 94
  תחומי מיומנות (ציונים מותאמים): תקשורת 5 · מושגים טרום-אקדמיים 7 · הכוונה עצמית 8 · פנאי 9
  חברתי 8 · תפקוד בבית / במסגרת 9 · בריאות ובטיחות 10 · טיפול עצמי 9 · מוטורי 10

CBCL 1½–5, דיווח הורים (ציוני T):
  תגובתיות רגשית 62 · חרדה/דיכאון 54 · תלונות סומטיות 51 · הסתגרות 60
  בעיות שינה 50 · בעיות קשב 58 · התנהגות תוקפנית 55
  מופנם 57 · מוחצן 54 · ציון כולל 58

ADOS-2, מודול 1:
  ציון השוואה 3
""")
    made.append("תמר/07 ציונים להקלדה.txt")
    text(os.path.join(TAMAR_DIR, "קרא אותי.txt"), TAMAR_README)


TAMAR_README = f"""
תיק בדוי לדמו: {TAMAR}, בת 3 ו-3 חודשים
=========================================

הכל בדוי: הילדה, המשפחה, המעון, המכונים והמספרים. אין כאן אף אדם אמיתי.

איך מתחילים:
1. בכספת: תיק חדש. בשלב "מי בתיק" מקלידים:
   ילדה: {TAMAR} {T_SURNAME} (שם פרטי ומשפחה באותה שורה) · אם: {T_MOTHER} · אב: {T_FATHER} · אח: {T_BROTHER}
   מטפלת/סייעת: {T_CARER} · מעון (גן): {T_NURSERY} · יישוב: {T_TOWN}
   קלינאית תקשורת: {T_SLP} · רופאה: {T_DOCTOR}
   (את "{T_FRIEND}" ואת סבתא "{T_GRANNY}" לא מכניסים בכוונה: התוכנה מסתירה אותן לבד.)
2. מייבאים את הקבצים 01–06.
3. מקלידים את הציונים מקובץ 07 במסך הציונים.

מה אמור לקרות:
- 01 (Word): השם, ת.ז., הטלפון, המייל והכתובת מוסתרים. הכותרת העליונה והתחתונה ושם הכותבת
  בתכונות הקובץ לא נכנסים לתיק.
- 02 (ODT): ההערה הפנימית של המטפלת והנוסח שנמחק ("על הספקטרום") לא נכנסים. "{T_FRIEND}" מוסתרת.
- 03 (Word): השורה עם שם הרופאה בתחתית העמוד לא נכנסת לגוף; בגוף השם שלה מוחלף בתגית.
- 04 (PDF): נקרא בעברית; השורות בראש העמוד ובתחתיתו (שם הקלינאית, השם ות.ז. של הילדה) לא נכנסות לגוף.
- 05 (מייל): "{T_GRANNY}" מוסתרת; הטלפון מוסתר.
- 06 (טקסט): השמות מוחלפים בתגיות לפני כל שליחה.
- בכל שליחה: במסך "מה יוצא מהמחשב" רואים בדיוק מה יוצא, ואף שם מהרשימה לא מופיע בו.
"""

README = f"""
מסמכי דוגמה בדויים לבדיקת כספת האבחון
========================================

הכל בדוי: הילד, המשפחה, הגן, המכונים והמספרים. אין כאן אף אדם אמיתי.

איך מתחילים:
1. בכספת: תיק חדש. בשלב "מי בתיק" מקלידים:
   ילד: {CHILD} (שם משפחה: {SURNAME}) · אם: {MOTHER} · אב: {FATHER} · אחות: {SISTER}
   גננת: {TEACHER} · גן: {GAN} · יישוב: {TOWN}
   קלינאית תקשורת: {SLP} · מרפאה בעיסוק: {OT} · רופא: {DOCTOR}
   (את "{STRANGER}" לא מכניסים בכוונה: התוכנה צריכה לשאול עליו.)
2. מייבאים את הקבצים 01–06 ורואים מה קורה (למטה).
3. מקלידים את הציונים מקובץ 07 במסך הציונים.
4. לאבחון מעקב: התיקייה "מעקב - שנה אחרי".

מה אמור לקרות:
- 01 (Word): השם, ת.ז., הטלפון, המייל והכתובת מוסתרים. הכותרת העליונה והתחתונה ושם הכותבת
  בתכונות הקובץ לא נכנסים לתיק; התוכנה מציעה להוסיף את השם לרשימה.
- 02 (ODT): ההערה הפנימית של הגננת והנוסח שנמחק ("הפרעת קשב") לא נכנסים. "{STRANGER}" עולה כשם חשוד.
- 03 (Word): הטבלה נשמרת; הכותרת עם שם הקלינאית לא נכנסת לגוף.
- 04 (PDF): נקרא בעברית בסדר הנכון; השורה עם ת.ז. בתחתית העמוד לא נכנסת לגוף.
- 05, 06 (טקסט): השמות מוחלפים בתגיות לפני כל שליחה.
- 08 (סרוק): התוכנה מסרבת ומסבירה שזו תמונה בלי טקסט.
- בכל שליחה: במסך הבדיקה רואים בדיוק מה יוצא, ואף שם אמיתי מהרשימה לא מופיע בו.
"""


def main():
    os.makedirs(FOLLOW, exist_ok=True)
    made = []
    first_round(made)
    follow_up(made)
    tamar(made)
    text(os.path.join(OUT, "קרא אותי.txt"), README)
    print(f"{len(made)} documents in {OUT}")
    for m in made:
        print("  " + m)
    if not any(m.endswith("ריפוי בעיסוק.pdf") for m in made):
        print("  (the PDF from text needs LibreOffice: it was skipped)")


if __name__ == "__main__":
    main()
