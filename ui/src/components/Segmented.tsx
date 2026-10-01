import type { ReactNode } from "react";

const SIZES = {
  header: { group: "rounded-md", option: "h-7 px-2.5 text-[11px] tracking-[0.05em]" },
  row: { group: "rounded-[11px]", option: "h-5 px-2 text-[10px] tracking-[0.03em]" },
} as const;

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
