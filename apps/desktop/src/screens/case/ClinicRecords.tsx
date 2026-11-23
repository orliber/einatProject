// On the case's materials page: the session records of the linked clinic card that were not
// copied to the case yet, and the step that brings them (D-217). It is here, where she
// collects what the report is written from, that she needs them. Quiet: a case without a
// card, a card with nothing to bring, or a clinic that cannot be read, show nothing.
import { useQuery } from "@tanstack/react-query";
import { clinicApi } from "../../ipc/clinic";
import { clinicKeys } from "../clinic/clinicData";
import { today } from "../clinic/format";
import { NotesToCase } from "../clinic/NotesToCase";

/** The clinic patient whose card is linked to the case, or `null`. */
export async function cardOfCase(caseId: string): Promise<string | null> {
  const rows = await clinicApi.patients(today());
  return rows.find((r) => r.patient.case_id === caseId)?.patient.id ?? null;
}

/** The query both the case's bar and this line read, so the card is looked up once. */
export const cardOfCaseQuery = (caseId: string) => ({ queryKey: ["clinic", "card-of-case", caseId], queryFn: () => cardOfCase(caseId), staleTime: 60_000 });

export function ClinicRecords({ caseId, onCopied }: { caseId: string; onCopied: () => void }) {
  const cardId = useQuery(cardOfCaseQuery(caseId)).data ?? null;
  return cardId === null ? null : <Records cardId={cardId} onCopied={onCopied} />;
}

function Records({ cardId, onCopied }: { cardId: string; onCopied: () => void }) {
  // A plain read, not the clinic's own: a clinic that cannot be read is not an error of the case.
  // Under the card's key, so a record copied here or on the card is read again in both places.
  const now = today();
  const card = useQuery({ queryKey: clinicKeys.card(cardId, now.date), queryFn: () => clinicApi.patientCard(cardId, now), retry: false });
  return card.data ? <NotesToCase card={card.data} inCase onCopied={onCopied} /> : null;
}
