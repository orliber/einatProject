# Gold corpus for the de-identification filter

Invented, complete documents of the kinds a developmental psychologist receives and writes.
**Everything here is fabricated.** No real child, parent, professional, kindergarten or
number appears, and none may be added. `tests/corpus.rs` strips the annotations, runs the
filter on each document, and prints recall, false hides and questions per group.

* `dev/` – used for tuning rules, lexicons and model weights.
* `test/` – **sealed.** Written separately, before the engine changed. Never tune anything
  against it. Only annotation mistakes may be fixed, with the reason in the commit message.

## File format

```
case: tamar
type: parent_email
declared: child=תמר דמיוני; mother=מיכל; father=עידו; brother=איתי
---
שלום, בשבוע שעבר {{תמר|decl}} שיחקה עם {{עלמה|name|other_child}} ...
```

Header lines (until `---`):

* `case:` – documents with the same case id belong to one case and share `declared`.
* `type:` – one of `intake_form`, `referral_form`, `kindergarten_report`, `parent_email`,
  `slp_summary`, `ot_summary`, `doctor_letter`, `observation_notes`, `psych_report`, `other`.
* `declared:` – what the psychologist typed in the case form, `role=value` separated by `;`.
  Roles are `dv_domain::Role` in snake_case: `child mother father brother sister teacher
  assistant doctor slp psychologist therapist other_child kindergarten school town
  institution other`. A role may repeat.

Body annotations: `{{surface|category}}` or `{{surface|category|role}}`.
Prefix letters stay outside the braces: `ל{{תמר|decl}}`, `ו{{אסתי|name|teacher}}`.

| category | must be | examples |
|---|---|---|
| `decl` | hidden | a declared identity, any spelling |
| `name` | hidden | an undeclared person's first name, nickname or full name (role required) |
| `surname` | hidden | a family name |
| `place` | hidden | locality, neighbourhood, street address |
| `inst` | hidden | a named kindergarten, school, clinic, hospital, workplace |
| `num` | hidden | ID, phone, e-mail, URL, health-fund or passport number, plate |
| `date` | hidden | a calendar date, Hebrew or Gregorian, or a year (ages and durations are not dates) |
| `indirect` | hidden or generalized | a parent's profession or workplace, a unique event |
| `keep` | untouched | an ordinary word that looks like a name ("אליה", "שני ההורים", "מתן זמן") |

Roles for `name`: `other_child relative teacher assistant doctor therapist professional other`.

Phone numbers and valid-looking ID numbers carry the join mark `¦` somewhere inside
(`054-¦1234567`) so the repository's pre-commit scan does not flag them; the harness removes
it. E-mail addresses use `example.com` only.
