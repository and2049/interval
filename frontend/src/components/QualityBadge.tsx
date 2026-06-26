import type { DataQuality } from "../../../shared/types/api";
import { badgeClass, qualityBadge } from "../lib/replayQuality";

export function QualityBadge(props: { quality: DataQuality; title?: string }) {
  const badge = () => qualityBadge(props.quality);

  return (
    <span
      class={`border px-1.5 py-0.5 font-mono text-[0.62rem] uppercase ${badgeClass(badge().tone)}`}
      title={props.title ?? "Section data quality"}
    >
      {badge().label}
    </span>
  );
}
