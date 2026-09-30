import type { ReactNode } from "react";

/**
 * The app's one segmented control: a bordered row of uppercase choices, the pressed ones filled.
 * `pressed` says which are pressed, so the same control serves one choice out of several (a range,
 * a filter) and independent ones (which lists to show).
 */
export function Segmented<T extends string>({
  options,
  pressed,
  onPress,
  ariaLabel,
  ariaLabelledBy,
  disabled = false,
}: {
  options: readonly { value: T; label: ReactNode; disabled?: boolean }[];
  pressed: (value: T) => boolean;
  onPress: (value: T) => void;
  ariaLabel?: string;
  ariaLabelledBy?: string;
  disabled?: boolean;
}) {
  return (
    <div
      className="inline-grid shrink-0 grid-flow-col overflow-hidden rounded-md border border-[var(--hair)]"
      role="group"
      aria-label={ariaLabel}
      aria-labelledby={ariaLabelledBy}
    >
      {options.map((option) => {
        const on = pressed(option.value);
        return (
          <button
            key={option.value}
            type="button"
            className={`h-7 cursor-pointer rounded-none border-0 px-2.5 text-[11px] font-semibold tracking-[0.05em] whitespace-nowrap uppercase focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-[var(--fill)] disabled:cursor-default ${
              // A pressed option that cannot be let go, such as the last list shown, stays filled.
              on ? "bg-[var(--fill)] text-[var(--fill-ink)]" : "bg-transparent text-[var(--mute)] disabled:opacity-45"
            }`}
            aria-pressed={on}
            disabled={disabled || option.disabled}
            onClick={() => onPress(option.value)}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
