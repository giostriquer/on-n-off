import "./Rocker.css";

type Size = "tab" | "plugin" | "skill" | "master" | "theme";

type RockerProps = {
  on: boolean;
  size?: Size;
  busy?: boolean;
  disabled?: boolean;
  dangerOff?: boolean;
  onLabel?: string;
  offLabel?: string;
  ariaLabel: string;
  onToggle: () => void;
};

export function Rocker({
  on = false,
  size = "plugin",
  busy = false,
  disabled = false,
  dangerOff = false,
  onLabel = "ON",
  offLabel = "OFF",
  ariaLabel,
  onToggle,
}: RockerProps) {
  const locked = disabled || busy;

  function activate() {
    if (locked) {
      return;
    }
    onToggle();
  }

  const className = [
    "rocker",
    size,
    on ? "on" : "",
    busy ? "busy" : "",
    dangerOff ? "dangerOff" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <button
      type="button"
      className={className}
      disabled={locked}
      aria-pressed={on}
      aria-label={ariaLabel}
      aria-busy={busy}
      onClick={activate}
    >
      <span className="half off">{offLabel}</span>
      <span className="half on-side">{onLabel}</span>
    </button>
  );
}
