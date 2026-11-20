// The help of the screen or window she is in (D-206), in place of the keyboard shortcuts sheet:
// what she can do here, a picture, tips and common questions; any other topic is one search or
// one click away, and the keyboard shortcuts are folded at the bottom. Everything here is static
// copy and pictures inside the program: nothing is fetched and nothing is sent.
import { useEffect, useMemo, useRef, useState } from "react";
import { Dialog } from "../components/ui";
import { ShortcutsList } from "../components/ShortcutsSheet";
import { GROUP_LABEL, searchTopics, topicOf, ALL_TOPICS } from "./topics";
import { shotUrl } from "./shots";
import type { HelpGroup, HelpId, HelpTopic } from "./types";
import "./HelpPanel.css";

export function HelpPanel({ start, onClose }: { start: HelpId; onClose: () => void }) {
  // The way she came, for "חזרה": the topic she opened the help on is first.
  const [trail, setTrail] = useState<HelpId[]>([start]);
  const [query, setQuery] = useState("");
  const [all, setAll] = useState(false);
  const id = trail[trail.length - 1] ?? start;
  const topic = topicOf(id) ?? topicOf("cases");
  const body = useRef<HTMLDivElement>(null);
  const found = useMemo(() => searchTopics(query), [query]);

  const open = (next: HelpId) => {
    setQuery("");
    setAll(false);
    setTrail((t) => (t[t.length - 1] === next ? t : [...t, next]));
  };
  // A new topic starts at its top, with the keyboard on its heading.
  useEffect(() => {
    const frame = body.current?.closest<HTMLElement>(".dialog-body");
    if (frame) frame.scrollTop = 0;
    if (trail.length > 1) body.current?.querySelector<HTMLElement>("h3")?.focus();
  }, [id, trail.length]);

  if (!topic) return null;
  const back = trail.length > 1 ? topicOf(trail[trail.length - 2] as HelpId) : undefined;
  const here = id === start;
  return (
    <Dialog title={`עזרה: ${topic.title}`} subtitle={here ? "מה אפשר לעשות במה שפתוח עכשיו" : GROUP_LABEL[topic.group]}
      onClose={onClose} helpPanel
      footer={<>
        {back && <button type="button" className="btn" onClick={() => setTrail((t) => t.slice(0, -1))}>→ חזרה ל{back.title}</button>}
        <span className="grow" />
        <button type="button" className="btn btn-primary" onClick={onClose}>סגירה</button>
      </>}>
      <div ref={body} className="help-panel">
        <div className="help-find" role="search">
          <label className="visually-hidden" htmlFor="help-query">חיפוש בעזרה</label>
          <input id="help-query" className="input" type="search" placeholder="חיפוש בעזרה, למשל: גיבוי, הסכמה, פגישה"
            value={query} onChange={(e) => setQuery(e.target.value)} />
          <button type="button" className="btn btn-quiet" aria-expanded={all} onClick={() => { setAll(!all); setQuery(""); }}>
            {all ? "לסגור את הרשימה" : "כל נושאי העזרה"}
          </button>
        </div>
        {query.trim() ? (
          <Results topics={found} onOpen={open} query={query} />
        ) : all ? (
          <AllTopics current={id} onOpen={open} />
        ) : (
          <TopicView topic={topic} onOpen={open} />
        )}
        <details className="help-keys">
          <summary>קיצורי מקלדת</summary>
          <ShortcutsList />
        </details>
      </div>
    </Dialog>
  );
}

function TopicView({ topic, onOpen }: { topic: HelpTopic; onOpen: (id: HelpId) => void }) {
  const src = shotUrl(topic.shot);
  const [big, setBig] = useState(false);
  return (
    <article className="help-topic" aria-labelledby="help-topic-title">
      <h3 id="help-topic-title" tabIndex={-1}>{topic.title}</h3>
      <p className="help-intro">{topic.intro}</p>
      <section aria-labelledby="help-can">
        <h4 id="help-can">מה אפשר לעשות כאן</h4>
        <ul className="help-can">
          {topic.can.map((c) => (
            <li key={c.title}><strong>{c.title}.</strong> {c.text}</li>
          ))}
        </ul>
      </section>
      {src && (
        <figure className={big ? "help-shot help-shot-big" : "help-shot"}>
          {/* A small picture next to the words; one click shows it at full width, another shrinks it. */}
          <button type="button" className="help-shot-btn" aria-pressed={big} onClick={() => setBig(!big)}
            title={big ? "להקטין את הצילום" : "להגדיל את הצילום"}>
            <img src={src} alt={topic.shotAlt ?? `צילום מסך: ${topic.title}`} loading="lazy" decoding="async" />
          </button>
          <figcaption className="small muted">צילום מתוך ההדגמה, כל השמות והפרטים בדויים. {big ? "לחיצה מקטינה." : "לחיצה מגדילה."}</figcaption>
        </figure>
      )}
      {topic.tips && topic.tips.length > 0 && (
        <section aria-labelledby="help-tips">
          <h4 id="help-tips">טיפים</h4>
          <ul className="help-tips">{topic.tips.map((t) => <li key={t}>{t}</li>)}</ul>
        </section>
      )}
      {topic.faq && topic.faq.length > 0 && (
        <section aria-labelledby="help-faq">
          <h4 id="help-faq">שאלות נפוצות</h4>
          <div className="help-faq">
            {topic.faq.map((f) => (
              <details key={f.q}>
                <summary>{f.q}</summary>
                <p>{f.a}</p>
              </details>
            ))}
          </div>
        </section>
      )}
      {topic.related && topic.related.length > 0 && (
        <section aria-labelledby="help-related">
          <h4 id="help-related">כדאי לקרוא גם</h4>
          <div className="help-links">
            {topic.related.map((r) => {
              const t = topicOf(r);
              return t ? <button key={r} type="button" className="btn btn-quiet" onClick={() => onOpen(r)}>{t.title} ←</button> : null;
            })}
          </div>
        </section>
      )}
    </article>
  );
}

function Results({ topics, onOpen, query }: { topics: HelpTopic[]; onOpen: (id: HelpId) => void; query: string }) {
  return (
    <section aria-label="תוצאות החיפוש" className="help-list">
      <p role="status" className="small muted">
        {topics.length === 0 ? `לא נמצא נושא עם "${query.trim()}". אפשר לנסות מילה אחרת, או לפתוח את כל נושאי העזרה.`
          : topics.length === 1 ? "נמצא נושא אחד." : `נמצאו ${topics.length} נושאים.`}
      </p>
      <ul>
        {topics.map((t) => (
          <li key={t.id}>
            <button type="button" className="help-list-item" onClick={() => onOpen(t.id)}>
              <strong>{t.title}</strong><span className="small muted">{GROUP_LABEL[t.group]} · {t.intro}</span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

function AllTopics({ current, onOpen }: { current: HelpId; onOpen: (id: HelpId) => void }) {
  const groups: HelpGroup[] = ["reports", "clinic", "settings"];
  return (
    <div className="help-all">
      {groups.map((g) => (
        <section key={g} aria-labelledby={`help-group-${g}`} className="help-list">
          <h4 id={`help-group-${g}`}>{GROUP_LABEL[g]}</h4>
          <ul className="help-chips">
            {ALL_TOPICS.filter((t) => t.group === g).map((t) => (
              <li key={t.id}>
                <button type="button" className="btn btn-quiet" aria-current={t.id === current ? "true" : undefined}
                  onClick={() => onOpen(t.id)}>{t.title}</button>
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
