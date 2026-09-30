import type { ReactNode } from "react";

/**
 * How big the control is, by where it sits: `header`, the default, beside a screen's or a section's
 * title or under a form field's caption; `row` in a settings card's row, where it matches the OFF/ON
 * toggle (`Rocker`'s `skill` size) beside it. Each is a whole set of classes, never one appended to
 * the other, since Tailwind settles a conflict by stylesheet order.
 */
const SIZES = {
  header: { group: "rounded-md", option: "h-7 px-2.5 text-[11px] tracking-[0.05em]" },
  row: { group: "rounded-[11px]", option: "h-5 px-2 text-[10px] tracking-[0.03em]" },
} as const;

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
  size = "header",
}: {
  options: readonly { value: T; label: ReactNode; disabled?: boolean }[];
  pressed: (value: T) => boolean;
  onPress: (value: T) => void;
  ariaLabel?: string;
  ariaLabelledBy?: string;
  disabled?: boolean;
  size?: keyof typeof SIZES;
}) {
  const { group, option: optionSize } = SIZES[size];
  return (
    <div
      className={`inline-grid shrink-0 grid-flow-col overflow-hidden border border-[var(--hair)] ${group}`}
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
            className={`${optionSize} cursor-pointer rounded-none border-0 font-semibold whitespace-nowrap uppercase focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-[var(--fill)] disabled:cursor-default ${
              // A pressed option that cannot be let go, such as the last list shown, stays filled;
              // the whole control disabled dims it with the rest.
              on
                ? `bg-[var(--fill)] text-[var(--fill-ink)] ${disabled ? "opacity-45" : ""}`
                : "bg-transparent text-[var(--mute)] disabled:opacity-45"
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
