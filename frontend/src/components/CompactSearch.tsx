import { PencilSimple } from "@phosphor-icons/react";

interface Props {
  origin: string | null;
  query: string | null;
  onEdit: () => void;
}

/** Une fois les résultats affichés : la recherche tient sur une ligne, un clic la rouvre. */
export default function CompactSearch({ origin, query, onEdit }: Props) {
  return (
    <button className="compact-search" onClick={onEdit} title="Modifier la recherche">
      <span className="compact-text">
        {origin && <strong>De {origin}</strong>}
        <span className="compact-query">{query ? `« ${query} »` : "Recherche par critères"}</span>
      </span>
      <span className="compact-edit">
        <PencilSimple size={16} aria-hidden /> Modifier
      </span>
    </button>
  );
}
