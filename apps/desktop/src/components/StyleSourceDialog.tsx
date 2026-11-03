import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { ipc, type StyleSourceView } from "../ipc/client";
import { dateOf } from "../screens/style/labels";
import { Dialog, ErrorLine, Skeleton } from "./ui";

/** A past report in "הסגנון שלי", opened to read: only the text that was kept, after the names
 *  and details were hidden. Plain text, never HTML. */
export function StyleSourceDialog({ source, onClose }: { source: StyleSourceView; onClose: () => void }) {
  const { fail } = useApp();
  const qc = useQueryClient();
  const text = useQuery({ queryKey: ["style-source", source.id], queryFn: () => ipc.styleSourceText(source.id), gcTime: 0 });
  return (
    <Dialog wide title={source.title} onClose={onClose}
      subtitle={`נוסף ב-${dateOf(source.added_at)} · ${source.words.toLocaleString("he-IL")} מילים · נשמר רק הטקסט אחרי הסתרה, לא הקובץ`}
      footer={<button type="button" className="btn btn-primary" onClick={onClose}>סגירה</button>}>
      {text.error
        ? <ErrorLine error={fail(text.error as never)} retry={() => void qc.invalidateQueries({ queryKey: ["style-source", source.id] })} />
        : !text.data
          ? <Skeleton lines={6} label="פותח את הדוח" />
          : (
            <article className="style-source-read">
              {text.data.parts.map((p, i) => (
                <section key={i} className="style-source-part">
                  <h3>{p.heading}</h3>
                  <p className="style-source-text">{p.text}</p>
                </section>
              ))}
            </article>
          )}
    </Dialog>
  );
}
