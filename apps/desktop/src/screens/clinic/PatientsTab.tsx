// The patient list: search (name, parent, phone), filter by where the family is, a sortable
// table with a row for each family. A row opens the card through the route, so "back" and a
// link from another tab land in the same place. Families that wait have their own tab; here
// they show only under "כולם", or when a search finds them.
import { useMemo, useState } from "react";
import { ageWords } from "../../components/AgeField";
import { Segmented } from "../../components/Switch";
import { PlusIcon, Skeleton } from "../../components/ui";
import type { PatientRow } from "../../ipc/generated/PatientRow";
import { useDebounced, usePatients, useReloadClinic, useToday } from "./clinicData";
import { CONTACT_ROLE_LABEL, formatDate, formatDateShort, formatTime, PATIENT_STATUS_LABEL, VISIT_KIND_LABEL } from "./format";
import { EmptyState, Money, Pager, PatientName, ReadError, useClinicNav, usePaged, useShownName } from "./parts";
import { PatientDialog } from "./PatientDialog";
import "./PatientsTab.css";

type Filter = "active" | "closed" | "all";
type SortKey = "name" | "age" | "next" | "last" | "due" | "tasks";
interface Sort {
  key: SortKey;
  down: boolean;
}

const FILTERS: { value: Filter; label: string }[] = [
  { value: "active", label: PATIENT_STATUS_LABEL.active },
  { value: "closed", label: PATIENT_STATUS_LABEL.closed },
  { value: "all", label: "כולם" },
];

const COLUMNS: { key: SortKey; label: string; numeric?: boolean }[] = [
  { key: "name", label: "שם" },
  { key: "age", label: "גיל" },
  { key: "next", label: "הפגישה הבאה" },
  { key: "last", label: "הפגישה האחרונה" },
  { key: "due", label: "יתרה לתשלום", numeric: true },
  { key: "tasks", label: "מעקבים פתוחים", numeric: true },
];

/** How many families a page of the list shows. */
const PAGE = 25;

const digits = (s: string) => s.replace(/\D/g, "");

/** What a search reads of a row: the child's name, the contacts' names, and their phones (as
 *  typed and as digits alone, so "0545550" finds "054-555-03"). */
function haystack(row: PatientRow): { text: string; phones: string } {
  const p = row.patient;
  return {
    text: [p.name, ...p.contacts.map((c) => c.name), ...p.contacts.map((c) => c.phone)].join("\n").toLowerCase(),
    phones: p.contacts.map((c) => digits(c.phone)).filter(Boolean).join("\n"),
  };
}

function matches(hay: { text: string; phones: string }, query: string): boolean {
  if (query === "") return true;
  if (hay.text.includes(query.toLowerCase())) return true;
  const number = digits(query);
  // Only a query that is a number is read as a phone ("יעל 3" is not).
  return number.length >= 3 && /^[\d\s()+-]+$/.test(query) && hay.phones.includes(number);
}

/** What a column sorts by; `null` (no birth date, no visit) always goes last. */
function sortValue(row: PatientRow, key: SortKey): string | number | null {
  switch (key) {
    case "name": return row.patient.name;
    // In months: the youngest child first.
    case "age": return row.age === null ? null : row.age.years * 12 + row.age.months;
    case "next": return row.next_visit === null ? null : `${row.next_visit.date} ${formatTime(row.next_visit.start)}`;
    case "last": return row.last_visit;
    case "due": return row.due;
    case "tasks": return row.open_tasks;
  }
}

function compare(a: PatientRow, b: PatientRow, sort: Sort): number {
  const x = sortValue(a, sort.key);
  const y = sortValue(b, sort.key);
  const byName = a.patient.name.localeCompare(b.patient.name, "he") || a.patient.id.localeCompare(b.patient.id);
  if (x === null || y === null) return x === y ? byName : x === null ? 1 : -1;
  const order = typeof x === "number" && typeof y === "number" ? x - y : String(x).localeCompare(String(y), "he");
  return (sort.down ? -order : order) || byName;
}

/** "היום, 09:00 · מפגש אבחון", "16.11, 10:00 · תצפית". */
function nextVisitWords(row: PatientRow, today: string): string {
  const next = row.next_visit;
  if (next === null) return "";
  return `${next.date === today ? "היום" : formatDateShort(next.date)}, ${formatTime(next.start)}`;
}

export function PatientsTab() {
  const now = useToday();
  const rows = usePatients(now.date);
  const nav = useClinicNav();
  const reload = useReloadClinic();
  const shown = useShownName();
  const [text, setText] = useState("");
  const query = useDebounced(text.trim(), 200);
  const [filter, setFilter] = useState<Filter>("active");
  const [sort, setSort] = useState<Sort>({ key: "name", down: false });
  const [adding, setAdding] = useState(false);

  // What a search reads is prepared once for each answer of the vault, not on every key.
  const indexed = useMemo(() => (rows.data ?? []).map((row) => ({ row, hay: haystack(row) })), [rows.data]);
  const found = useMemo(() => indexed.filter((x) => matches(x.hay, query)).map((x) => x.row), [indexed, query]);
  const list = useMemo(
    () => found.filter((r) => filter === "all" || r.patient.status === filter).sort((a, b) => compare(a, b, sort)),
    [found, filter, sort],
  );
  const paged = usePaged(list, PAGE);

  if (rows.error && !rows.data) return <ReadError error={rows.error.message} retry={reload} />;
  if (!rows.data) {
    return (
      <div className="stack patients" aria-busy="true">
        <Skeleton lines={1} label="טוען את החיפוש" />
        <Skeleton card lines={6} label="טוען את רשימת המטופלים" />
      </div>
    );
  }

  const all = rows.data;
  /** Families the search found that the filter is hiding (on the waiting list, or ended). */
  const elsewhere = found.length - list.length;

  function sortBy(key: SortKey, numeric: boolean) {
    // A second press on the same column turns it over; amounts and counts start from the largest.
    setSort((s) => (s.key === key ? { key, down: !s.down } : { key, down: numeric }));
    paged.setPage(0);
  }

  return (
    <div className="stack patients">
      <header className="patients-head">
        <div className="field patients-search">
          <label htmlFor="patients-search">חיפוש</label>
          <input id="patients-search" type="search" className="input" value={text} autoComplete="off" placeholder="שם הילד, שם של הורה או טלפון"
            onChange={(e) => { setText(e.target.value); paged.setPage(0); }} />
        </div>
        <div className="field">
          <span className="label">מה להציג</span>
          <Segmented label="מה להציג" value={filter} options={FILTERS} onChange={(f) => { setFilter(f); paged.setPage(0); }} />
        </div>
        <span className="grow" />
        <button type="button" className="btn btn-primary" onClick={() => setAdding(true)}><PlusIcon /> מטופל חדש</button>
      </header>

      {all.length === 0 ? (
        <EmptyState title="עוד אין מטופלים בקליניקה"
          actions={<button type="button" className="btn btn-primary" onClick={() => setAdding(true)}>להוסיף מטופל</button>}>
          <p>כל משפחה מקבלת כרטיס: הפרטים, הפגישות, הסיכומים והתשלומים במקום אחד.</p>
        </EmptyState>
      ) : (
        <>
          <p className="muted small patients-count" role="status">
            {list.length === 0 ? "" : list.length === 1 ? "מטופל אחד" : `${list.length} מטופלים`}
            {list.length > 0 && elsewhere > 0 && query !== "" ? ` · ${elsewhere === 1 ? "עוד אחד נמצא" : `עוד ${elsewhere} נמצאו`} מחוץ לסינון הזה` : ""}
          </p>

          {list.length === 0 ? (
            <EmptyState title={query !== "" ? "לא נמצא מטופל כזה" : filter === "closed" ? "אין כרטיסים שהסתיימו" : "אין כרגע מטופלים בתהליך"}
              actions={query !== "" && elsewhere > 0
                ? <button type="button" className="btn" onClick={() => { setFilter("all"); paged.setPage(0); }}>להציג את כולם</button>
                : query !== "" ? <button type="button" className="btn" onClick={() => { setText(""); paged.setPage(0); }}>לנקות את החיפוש</button>
                : filter === "active" ? <button type="button" className="btn" onClick={() => nav.openTab("waiting")}>לרשימת ההמתנה</button>
                : undefined}>
              <p>
                {query !== "" && elsewhere > 0
                  ? (elsewhere === 1 ? "יש כרטיס אחד שמתאים לחיפוש, אבל הוא לא בסינון הזה (ברשימת ההמתנה או בין הכרטיסים שהסתיימו)." : `יש ${elsewhere} כרטיסים שמתאימים לחיפוש, אבל הם לא בסינון הזה.`)
                  : query !== "" ? "אפשר לחפש לפי שם הילד, שם של אחד ההורים או מספר טלפון."
                  : filter === "closed" ? "כרטיס שנסגר נשאר שמור, ומופיע כאן."
                  : "משפחות שעוד ממתינות נמצאות ברשימת ההמתנה."}
              </p>
            </EmptyState>
          ) : (
            <div className="card patients-table-wrap">
              <table className="patients-table">
                <caption className="visually-hidden">רשימת המטופלים. לחיצה על כותרת של עמודה ממיינת לפיה.</caption>
                <thead>
                  <tr>
                    {COLUMNS.map((c) => (
                      <th key={c.key} scope="col" aria-sort={sort.key === c.key ? (sort.down ? "descending" : "ascending") : "none"}
                        className={c.numeric ? "patients-num" : undefined}>
                        <button type="button" className="patients-sort" onClick={() => sortBy(c.key, c.numeric === true)}>
                          {c.label}
                          <span className="patients-sort-mark" aria-hidden="true">{sort.key === c.key ? (sort.down ? "▼" : "▲") : ""}</span>
                        </button>
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {paged.shown.map((row) => {
                    const p = row.patient;
                    const parents = p.contacts.filter((c) => c.name !== "").slice(0, 2).map((c) => `${CONTACT_ROLE_LABEL[c.role]}: ${shown(c.name)}`);
                    return (
                      // The whole row opens the card for the mouse; the name is the button for the keyboard.
                      <tr key={p.id} className="patients-row" onClick={(e) => { if (!(e.target as HTMLElement).closest("button")) nav.openPatient(p.id); }}>
                        <th scope="row">
                          <div className="stack patients-who">
                            <span className="row patients-name">
                              <PatientName name={p.name} id={p.id} strong />
                              {p.status !== "active" && <span className="chip chip-sand">{PATIENT_STATUS_LABEL[p.status]}</span>}
                              {p.status === "waiting" && p.urgent && <span className="chip chip-warn">דחוף</span>}
                            </span>
                            {(parents.length > 0 || p.framework) && (
                              <span className="muted small patients-more">{[...parents, p.framework].filter(Boolean).join(" · ")}</span>
                            )}
                          </div>
                        </th>
                        <td>{row.age ? ageWords(row.age) : <span className="muted">לא צוין</span>}</td>
                        <td>
                          {row.next_visit
                            ? (
                              <span className="stack patients-visit">
                                <span className="num">{nextVisitWords(row, now.date)}</span>
                                <span className="muted small">{VISIT_KIND_LABEL[row.next_visit.kind]}</span>
                              </span>
                            )
                            : <span className="muted">אין פגישה ביומן</span>}
                        </td>
                        <td>{row.last_visit ? <span className="num">{formatDate(row.last_visit)}</span> : <span className="muted">עוד לא הייתה</span>}</td>
                        <td className="patients-num">
                          {row.due > 0 ? <Money agorot={row.due} tone="owed" />
                            : row.due < 0 ? <span className="muted">זכות <Money agorot={-row.due} tone="muted" /></span>
                            : <span className="muted">אין</span>}
                        </td>
                        <td className="patients-num">{row.open_tasks > 0 ? <span className="chip">{row.open_tasks}</span> : <span className="muted">אין</span>}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
          <Pager page={paged.page} pages={paged.pages} onPage={paged.setPage} label="עמודי רשימת המטופלים" />
        </>
      )}

      {adding && <PatientDialog onClose={() => setAdding(false)} onSaved={(p) => { if (p.status !== "waiting") nav.openPatient(p.id); }} />}
    </div>
  );
}
