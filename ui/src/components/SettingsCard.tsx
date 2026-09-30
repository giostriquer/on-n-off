import type { ReactNode } from "react";
import { Rocker, type RockerProps } from "./Rocker";

// Each constant is a whole variant, never a base to append to: Tailwind resolves two classes that
// set the same property by their order in its stylesheet, not in the class string, so an appended
// override can silently lose.

/** The small uppercase caption over a card's switch, and over a row's field. */
export const caption = "text-[10px] font-semibold tracking-[0.05em] text-[var(--mute)] uppercase";
/** A row's label: what the row's control sets. */
export const rowLabel = "min-w-0 flex-1 text-[12px] text-[var(--mute)]";
/** A row that names a thing, such as a provider, rather than a setting. */
export const rowName = "min-w-0 flex-1 text-[12px] text-[var(--silkscreen)]";
const buttonShape =
  "inline-flex h-8 items-center justify-center gap-1.5 rounded-md border px-2.5 text-[10px] font-semibold tracking-[0.04em] uppercase disabled:opacity-45";
/** A card's action button, its primary one, and a square one holding only an icon. */
export const cardButton = `${buttonShape} border-[var(--hair)]`;
export const cardButtonPrimary = `${buttonShape} border-[var(--fill)] bg-[var(--fill)] text-[var(--fill-ink)]`;
export const cardIconButton =
  "inline-flex size-8 shrink-0 items-center justify-center rounded-md border border-[var(--hair)] disabled:opacity-45";
/** A card's choice of one value out of a short list. */
export const cardSelect = "h-8 rounded-md border border-[var(--hair)] bg-[var(--well)] px-2 text-[11px] font-semibold";

/**
 * The card every setting sits on: a plate named for the setting (its title, unless `label` says
 * otherwise), its title and what it does, the card's own switch at the right of its header
 * (`CardToggle`), and rows below (`SettingRow`). `meta` is a line of data, such as a version or a
 * CLI's path, set in the mono face.
 */
export function SettingsCard({ label, title, description, meta, icon, control, as = "section", children }: {
  label?: string;
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
    <Card
      aria-label={label ?? (typeof title === "string" ? title : undefined)}
      className="rounded-[11px] border border-[var(--hair)] bg-[var(--plate)]"
    >
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

/**
 * One row of a card below its header: a label at the left and its control at the right, or, when
 * `stack`ed, a label over a control as wide as the card.
 */
export function SettingRow({ children, stack = false }: { children: ReactNode; stack?: boolean }) {
  return (
    <div
      className={`border-t border-[var(--hair)] px-3.5 py-2.5 ${
        stack ? "flex flex-col gap-2" : "flex flex-wrap items-center gap-3"
      }`}
    >
      {children}
    </div>
  );
}

/** A setting offered outside a card, in a menu or a panel: what it does, then its OFF/ON toggle. */
export function SwitchRow({ label, ...toggle }: { label: string } & Omit<RockerProps, "size" | "ariaLabel">) {
  return (
    <div className="flex items-center gap-3">
      <span className={rowName}>{label}</span>
      <Rocker size="skill" ariaLabel={label} {...toggle} />
    </div>
  );
}
