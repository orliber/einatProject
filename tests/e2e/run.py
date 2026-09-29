"""End-to-end run of the real app (demo mode, fabricated data) as a psychologist would use it.

Starts Xvfb + tauri-driver, drives the WebView over WebDriver, and saves a screenshot per step.
Linux only. Needs: xvfb, webkit2gtk-driver, tauri-driver (cargo install tauri-driver),
libreoffice-writer (to make the PDF fixture), and `pip install selenium msoffcrypto-tool`.
Build first: `cd apps/desktop && pnpm tauri build --debug --no-bundle`. Run: `python3 tests/e2e/run.py`.
Everything it writes (a fresh HOME, screenshots) goes to tests/e2e/out/, which git ignores.
"""
import os
import subprocess
import sys
import time
import traceback

from selenium import webdriver
from selenium.webdriver.common.action_chains import ActionChains
from selenium.webdriver.common.keys import Keys
from selenium.webdriver.common.by import By
from selenium.webdriver.common.options import ArgOptions
from selenium.webdriver.support.ui import WebDriverWait

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
APP = os.path.join(ROOT, "target", "debug", "dv-desktop")
OUT = os.path.join(HERE, "out")
HOME = os.path.join(OUT, "home")
SHOTS = os.path.join(OUT, "shots")
FIX = os.path.join(OUT, "fixtures")
os.makedirs(SHOTS, exist_ok=True)
os.makedirs(FIX, exist_ok=True)

# Fabricated fixtures, made fresh each run (no binary files in the repo).
DOCX = os.path.join(FIX, "סיכום קלינאית תקשורת.docx")
subprocess.run([sys.executable, os.path.join(HERE, "fixtures", "make_docx.py"), DOCX], check=True)
subprocess.run(["soffice", "--headless", "--convert-to", "pdf", "--outdir", FIX,
                os.path.join(HERE, "fixtures", "report.html")], check=True, capture_output=True)
PDF = os.path.join(FIX, "report.pdf")

env = dict(os.environ)
env.update(
    DISPLAY=":99",
    HOME=HOME,
    XDG_DATA_HOME=os.path.join(HOME, ".local/share"),
    XDG_DOWNLOAD_DIR=os.path.join(HOME, "Downloads"),
    XDG_CONFIG_HOME=os.path.join(HOME, ".config"),
    # Debug builds only: stands in for the system "Save as" / "Open" window (D-024).
    DV_E2E_DIALOG_DIR=os.path.join(OUT, "backup-drive"),
)
os.makedirs(os.path.join(HOME, "Downloads"), exist_ok=True)
os.makedirs(env["DV_E2E_DIALOG_DIR"], exist_ok=True)

xvfb = subprocess.Popen(["Xvfb", ":99", "-screen", "0", "1280x800x24"], stderr=subprocess.DEVNULL)
time.sleep(1)
driver_proc = subprocess.Popen(
    ["tauri-driver", "--native-driver", "/usr/bin/WebKitWebDriver"], env=env,
    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
)
time.sleep(2)

opts = ArgOptions()
opts.set_capability("tauri:options", {"application": APP})
opts.set_capability("browserName", "wry")
d = webdriver.Remote("http://127.0.0.1:4444", options=opts)
from selenium.webdriver.remote.file_detector import UselessFileDetector
d.file_detector = UselessFileDetector()
wait = WebDriverWait(d, 40)
step = [0]
log = []


def shot(name):
    step[0] += 1
    path = os.path.join(SHOTS, f"{step[0]:02d}-{name}.png")
    d.save_screenshot(path)
    log.append(f"ok   {step[0]:02d} {name}")


def find(xpath, timeout=40):
    return WebDriverWait(d, timeout).until(lambda drv: drv.find_element(By.XPATH, xpath))


def button(text, timeout=40):
    xp = f"//button[normalize-space()='{text}' or contains(normalize-space(), '{text}')]"
    el = WebDriverWait(d, timeout).until(lambda drv: next((b for b in drv.find_elements(By.XPATH, xp) if b.is_displayed() and b.is_enabled()), None))
    return el


def by_label(text):
    lab = find(f"//label[contains(normalize-space(), '{text}')]")
    target = lab.get_attribute("for")
    return d.find_element(By.ID, target) if target else lab.find_element(By.XPATH, ".//input|.//textarea|.//select")


def type_into(el, text):
    el.clear()
    el.send_keys(text)


def expect_text(text, timeout=40):
    WebDriverWait(d, timeout).until(lambda drv: text in drv.find_element(By.TAG_NAME, "body").text)


ok = True
try:
    # 1. First run.
    pw = find("//input[@id='pw1']", 60)
    shot("setup-password")
    pw.send_keys("כלב ירוק רץ מהר בגינה")
    d.find_element(By.ID, "pw2").send_keys("כלב ירוק רץ מהר בגינה")
    button("יצירת הכספת").click()
    grid = find("//div[contains(@class,'kit-grid')]", 90)
    groups = [s.text for s in grid.find_elements(By.TAG_NAME, "span")]
    key = "-".join(groups)
    shot("setup-recovery-kit")
    d.find_element(By.ID, "typed").send_keys(key)
    button("המשך").click()
    find("//textarea[@id='names']").send_keys("רותם בדויה, מכון בדוי להתפתחות")
    shot("setup-names")
    button("סיום וכניסה").click()

    # 2. Cases: first case.
    expect_text("התיק הראשון")
    shot("cases-empty")
    button("פתיחת תיק").click()
    find("//input[@id='name-0']").send_keys("נועם")
    d.find_element(By.ID, "alias-0").send_keys("נועמי")
    d.find_element(By.ID, "name-1").send_keys("דנה")
    d.find_element(By.ID, "name-2").send_keys("יוסי")
    d.find_element(By.ID, "nc-years").send_keys("5")
    type_into(d.find_element(By.ID, "nc-months"), "4")
    button("+ הוספת אדם").click()
    sel = find("//select[@id='role-3']")
    for o in sel.find_elements(By.TAG_NAME, "option"):
        if o.get_attribute("value") == "teacher":
            o.click()
    d.find_element(By.ID, "name-3").send_keys("מיכל")
    find("//div[contains(@class,'consent')]//input[@type='checkbox']").click()
    shot("new-case")
    button("פתיחת התיק").click()

    # 3. Materials: import a Word document and a PDF.
    expect_text("עוד אין חומרים בתיק")
    shot("materials-empty")
    file_input = find("//input[@type='file']")
    d.execute_script("arguments[0].hidden = false;", file_input)
    file_input.send_keys(DOCX)
    d.execute_script("arguments[0].hidden = true;", file_input)
    expect_text("ייבוא מסמך", 60)
    time.sleep(0.5)
    shot("import-docx")
    body = d.find_element(By.TAG_NAME, "body").text
    log.append("     import docx: kind suggested = " + ("דוח של איש מקצוע" if "דוח של איש מקצוע" in body else "?"))
    log.append("     import docx: header name offered = " + str("יעל בדויה" in body))
    hide_buttons = [b for b in d.find_elements(By.XPATH, "//button[normalize-space()='להסתיר']") if b.is_displayed()]
    for b in hide_buttons:
        b.click()
        time.sleep(0.3)
    button("שמירה בתיק").click()
    expect_text("המסמך נשמר בתיק")
    file_input = find("//input[@type='file']")
    d.execute_script("arguments[0].hidden = false;", file_input)
    file_input.send_keys(PDF)
    d.execute_script("arguments[0].hidden = true;", file_input)
    expect_text("ייבוא מסמך", 60)
    time.sleep(0.5)
    shot("import-pdf")
    button("שמירה בתיק").click()
    time.sleep(1)

    # Scores: WPPSI-IV with ranges from the fixed table.
    button("הזנת ציונים").click()
    for k, v in [("fsiq", "102"), ("vci", "112"), ("vsi", "104"), ("fri", "106"), ("wmi", "95"), ("psi", "84")]:
        find(f"//input[@id='score-{k}']").send_keys(v)
    time.sleep(1)
    shot("scores")
    log.append("     scores: preview has range + percentile = " + str("ציון 112, אחוזון 79 – ממוצע גבוה" in d.find_element(By.TAG_NAME, "body").text))
    button("שמירה בתיק").click()
    expect_text("הציונים נשמרו בתיק")

    # A session note with an undeclared name.
    button("רישום מפגש").click()
    find("//textarea[@id='w-text']").send_keys(
        "במפגש השני נועם הגיע בשמחה ונפרד מדנה בקלות. בפינת הבנייה סיפר שהוא משחק בגן בעיקר עם יובל. "
        "התקשה לספר רצף אירועים ונעזר בתמונות. כשביקשתי לעבור למשחק אחר, התנגד ונרגע אחרי כשתי דקות."
    )
    shot("session-note")
    button("שמירה בתיק").click()
    time.sleep(1)
    shot("materials")

    # 3b. Sorting the materials into sections (D-022), through the review screen (demo mode).
    button("מיון החומרים לסעיפים").click()
    expect_text("לפני שליחה ל-Claude", 60)
    time.sleep(0.5)
    body = d.find_element(By.TAG_NAME, "body").text
    log.append("     sorting review: unknown name 'יובל' flagged = " + str("יובל" in body and "לא מופיע" in body))
    for _ in range(8):
        pending = [b for b in d.find_elements(By.XPATH, "//button[normalize-space()='להסתיר את השם' or normalize-space()='זה לא שם, להשאיר' or normalize-space()='מילה רגילה, להשאיר']") if b.is_displayed()]
        if not pending:
            break
        pending[0].click()
        time.sleep(1)
    shot("sort-review")
    button("שליחה").click()
    expect_text("לסעיפים", 60)
    time.sleep(1)
    body = d.find_element(By.TAG_NAME, "body").text
    log.append("     sorting: materials sorted = " + str("מוינו לסעיפים" in body or "מוין לסעיפים" in body))
    shot("sorted")

    # 4. A section: draft from the sources, with the review screen.
    section = find("//button[contains(@class,'side-section')][.//span[normalize-space()='איכויות התקשורת']]")
    d.execute_script("arguments[0].scrollIntoView({block: 'center'});", section)
    section.click()
    button("טיוטה מהחומרים").click()
    expect_text("לפני שליחה ל-Claude", 60)
    time.sleep(0.5)
    shot("review-suspect")
    body = d.find_element(By.TAG_NAME, "body").text
    log.append("     section review: 'יובל' already decided in the sorting review = " + str("מי זה" not in body))
    hide = [b for b in d.find_elements(By.XPATH, "//button[normalize-space()='להסתיר את השם']") if b.is_displayed()]
    for b in hide[:3]:
        b.click()
        time.sleep(1)
    # Any remaining suspects (e.g. ambiguous words): keep as ordinary words.
    for _ in range(5):
        rest = [b for b in d.find_elements(By.XPATH, "//button[normalize-space()='זה לא שם, להשאיר' or normalize-space()='מילה רגילה, להשאיר']") if b.is_displayed()]
        if not rest:
            break
        rest[0].click()
        time.sleep(1)
    shot("review-cleared")
    button("שליחה").click()
    expect_text("טיוטת הסעיף", 60)
    time.sleep(1.5)
    shot("section-draft")
    approve = [b for b in d.find_elements(By.XPATH, "//button[normalize-space()='אישור']") if b.is_displayed()]
    if approve:
        approve[0].click()
        time.sleep(1)
    shot("section-approved")

    # 5. Export (from the materials view, with the approved paragraph in the report).
    button("הפקת דוח Word").click()
    expect_text("הפקת דוח Word")
    time.sleep(1)
    shot("export")
    export_password = find("//input[@id='exp-pw']").get_attribute("value")
    button("הפקת הדוח").click()
    expect_text("הדוח מוכן", 60)
    shot("export-done")
    found = [os.path.join(r, f) for r, _, fs in os.walk(HOME) for f in fs if f.endswith(".docx")]
    log.append("     export: saved = " + ", ".join(p.replace(HOME, "~") for p in found))
    import io, zipfile, msoffcrypto
    with open(found[0], "rb") as fh:
        office = msoffcrypto.OfficeFile(fh)
        office.load_key(password=export_password, verify_password=True)
        plain = io.BytesIO()
        office.decrypt(plain, verify_integrity=True)
    xml = zipfile.ZipFile(plain).read("word/document.xml").decode()
    log.append("     export: encrypted, opens with the password, integrity ok")
    log.append("     export: names restored = " + str("נועם" in xml) + ", tags left = " + str("[ילד]" in xml or "[אם]" in xml))
    log.append("     export: author in metadata = " + str("creator" in zipfile.ZipFile(plain).read("docProps/core.xml").decode()))
    button("סגירה").click()

    # 6. Consultation.
    button("התייעצות").click()
    find("//textarea[@id='consult-q']").send_keys("מה ההבדל בין WPPSI-IV ל-WISC-V בגיל 6?")
    button("שליחה").click()
    expect_text("לפני שליחה ל-Claude")
    time.sleep(0.5)
    shot("consult-review")
    find("//section[@role='dialog']//button[contains(normalize-space(),'שליחה')]").click()
    expect_text("הדגמה", 60)
    time.sleep(0.5)
    shot("consult")

    # 7. Settings.
    button("הגדרות").click()
    expect_text("חיבור ל-Claude")
    shot("settings")

    # Cases list with the case in progress.
    button("תיקים").click()
    expect_text("תיקים בעבודה")
    time.sleep(0.5)
    shot("cases-list")

    # 7b. Folders (D-023): a new folder, the case moved into it from the "⋯" menu.
    button("תיקייה חדשה").click()
    find("//input[@id='folder-name']").send_keys("אבחונים פרטיים")
    button("שמירה").click()
    # By DOM text: WebKit's driver leaves ellipsis-clipped text out of .text.
    find("//span[contains(@class,'folder-name')][normalize-space()='אבחונים פרטיים']")
    # From the keyboard, as someone without a mouse would (Enter opens, arrows move).
    find("//button[starts-with(@aria-label, 'פעולות על התיק של')]").send_keys(Keys.ENTER)
    time.sleep(0.4)
    item = find("//*[@role='menuitem'][contains(normalize-space(), 'העברה לתיקייה')]")
    ActionChains(d).send_keys(Keys.ARROW_DOWN).perform()
    time.sleep(0.2)
    log.append("     folders: actions menu opens from the keyboard = " + str(item.is_displayed()))
    ActionChains(d).send_keys(Keys.ENTER).perform()
    time.sleep(0.4)
    find("//label[contains(@class,'move-row')][.//span[normalize-space()='אבחונים פרטיים']]").click()
    find("//section[@role='dialog']//button[normalize-space()='העברה']").click()
    expect_text("התיק הועבר")
    find("//button[contains(@class,'folder-open')]").click()
    time.sleep(0.5)
    body = d.find_element(By.TAG_NAME, "body").text
    here = d.find_elements(By.XPATH, "//nav[@aria-label='מיקום']//*[@aria-current='page'][normalize-space()='אבחונים פרטיים']")
    log.append("     folders: case inside the new folder = " + str("נועם" in body and len(here) == 1))
    shot("folder")

    # 7c. Backup (D-024): the reminder on the cases screen, then a restore drill in settings.
    button("כל התיקים").click()
    find("//div[contains(@class,'backup-reminder')]")
    shot("backup-reminder")
    find("//div[contains(@class,'backup-reminder')]//button[contains(normalize-space(),'גיבוי עכשיו')]").click()
    expect_text("הגיבוי נשמר מוצפן")
    drive = env["DV_E2E_DIALOG_DIR"]
    files = [f for f in os.listdir(drive) if f.endswith(".vaultbak")]
    with open(os.path.join(drive, files[0]), "rb") as fh:
        raw = fh.read()
    log.append(f"     backup: {files[0]} ({len(raw)} bytes), readable names = "
               + str(any(n.encode() in raw for n in ["נועם", "דנה", "יוסי", "SQLite format"])))
    # The toast sits over the top bar for a few seconds.
    WebDriverWait(d, 15).until(lambda drv: not drv.find_elements(By.CLASS_NAME, "toast"))
    button("הגדרות").click()
    expect_text("הגיבוי האחרון: היום")
    button("בדיקת גיבוי").click()
    find("//input[@id='check-pw']").send_keys("כלב ירוק רץ מהר בגינה")
    find("//section[@role='dialog']//button[normalize-space()='בדיקה']").click()
    expect_text("הגיבוי נפתח ותקין", 60)
    time.sleep(0.3)
    shot("backup-drill")
    vault_dir = os.path.join(HOME, ".local/share", "il.diagnosticvault.desktop", "vault")
    log.append("     backup: drill opened it, one case = " + str("תיק אחד" in d.find_element(By.TAG_NAME, "body").text)
               + ", scratch left next to the vault = " + str(any(n.startswith(".") for n in os.listdir(vault_dir))))
    find("//section[@role='dialog']//button[normalize-space()='סגירה']").click()

    # 8. Lock.
    button("נעילה").click()
    find("//input[@id='pw']")
    shot("locked")
except Exception:
    ok = False
    log.append("FAIL " + " / ".join(traceback.format_exc().splitlines()[-4:]))
    try:
        shot("failure")
        log.append("     page text: " + d.find_element(By.TAG_NAME, "body").text[:600].replace("\n", " | "))
    except Exception:
        pass
finally:
    try:
        d.quit()
    except Exception:
        pass
    driver_proc.terminate()
    xvfb.terminate()
    print("\n".join(log))
    sys.exit(0 if ok else 1)
