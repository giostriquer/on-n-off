import type { ReactNode } from "react";
import { Rocker, type RockerProps } from "./Rocker";

/** The small uppercase caption over a card's switch, and over a row's field. */
export const caption = "text-[10px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase";
/** A row's label: what the row's control sets. */
export const rowLabel = "min-w-0 flex-1 text-[12px] text-[var(--mute)]";
/** A card's action button, and its primary one. */
export const cardButton =
  "inline-flex h-8 items-center justify-center gap-1.5 rounded-md border border-[var(--hair)] px-2.5 text-[10px] font-semibold tracking-[0.04em] uppercase disabled:opacity-45";
export const cardButtonPrimary = `${cardButton} border-[var(--fill)] bg-[var(--fill)] text-[var(--fill-ink)]`;
/** A card's choice of one value out of a short list. */
export const cardSelect = "h-8 rounded-md border border-[var(--hair)] bg-[var(--well)] px-2 text-[11px] font-semibold";

/**
 * The card every setting sits on: a plate named for the setting, its title and what it does, the
 * card's own switch at the right of its header (`CardToggle`), and rows below (`SettingRow`).
 * `meta` is a line of data, such as a version or a CLI's path, set in the mono face.
 */
export function SettingsCard({ label, title, description, meta, icon, control, as = "section", children }: {
  label: string;
  title: ReactNode;
  description?: ReactNode;
  meta?: ReactNode;
  icon?: ReactNode;
  control?: ReactNode;
  as?: "section" | "article";
  children?: ReactNode;
}) {
  const Card = as;
  return (
    <Card aria-label={label} className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]">
      <div className="flex flex-wrap items-start gap-3 px-3.5 py-3">
        {icon}
        <div className="min-w-0 flex-1">
          <h3 className="m-0 text-[13px] font-semibold">{title}</h3>
          {description ? <p className="mt-1 mb-0 text-[12px] text-[var(--mute)]">{description}</p> : null}
          {meta ? <p className="mt-1 mb-0 font-mono text-[12px] text-[var(--mute)]">{meta}</p> : null}
        </div>
        {control}
      </div>
      {children}
    </Card>
  );
}

/** A card's own switch: its caption over the OFF/ON toggle, at the right of the card's header. */
export function CardToggle({ caption: text, ...toggle }: { caption: string } & Omit<RockerProps, "size">) {
  return (
    <div className="flex flex-col items-end gap-1">
      <span className={caption}>{text}</span>
      <Rocker size="skill" {...toggle} />
    </div>
  );
}

/** One row of a card below its header: a label at the left and its control at the right. */
export function SettingRow({ children, className = "" }: { children: ReactNode; className?: string }) {
  return (
    <div className={`flex flex-wrap items-center gap-3 border-t border-[var(--hair)] px-3.5 py-2.5 ${className}`}>
      {children}
    </div>
  );
}
