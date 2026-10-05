# OCR test scans

An invented referral letter (no real person; the names are those of `tests/fixtures/fake_case_noam.yaml`),
rendered as page images for the OCR tests in `../ocr.rs`:

- `scan-letter.png` – A4 at 300 dpi, 8-bit gray.
- `scan-letter-sideways.png` – the same page turned 90°, as a phone photo often arrives.
- `scan-letter-fax.tif` – black and white, fax (CCITT G4) compressed, as most office scanners save to PDF.

Made with ImageMagick from the text below (DejaVu Sans, 50 pt), never from a real document:

```
מכתב הפניה לאבחון פסיכולוגי
נועם בן חמש נבדק אצלי במרפאה בכפר ורדים.
ההורים רותם ועידו מדווחים על קושי במעברים בגן הדקל.
הגננת מיכל ציינה שהוא משחק לבד בחצר.
בבדיקה הגופנית לא נמצאו ממצאים חריגים.
בברכה, ד"ר אבנר שטרן
```
