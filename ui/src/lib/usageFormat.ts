/** Display formatting for the Usage screen. */

const CURRENCY = new Intl.NumberFormat("en-US", {
  style: "currency",
  currency: "USD",
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});

const INTEGER = new Intl.NumberFormat("en-US");

const HOUR_MS = 60 * 60 * 1000;

export function formatUsd(value: number): string {
  return CURRENCY.format(value);
}

export function formatCount(value: number): string {
  return INTEGER.format(Math.round(value));
}

export function formatTokens(value: number): string {
  const abs = Math.abs(value);
  if (abs >= 1e12) return `${trim(value / 1e12)}T`;
  if (abs >= 1e9) return `${trim(value / 1e9)}B`;
  if (abs >= 1e6) return `${trim(value / 1e6)}M`;
  if (abs >= 1e3) return `${trim(value / 1e3)}K`;
  return INTEGER.format(Math.round(value));
}

function trim(value: number): string {
  const abs = Math.abs(value);
  const digits = abs >= 100 ? 0 : abs >= 10 ? 1 : 2;
  return value.toFixed(digits).replace(/\.0+$/, "");
}

export function formatPercent(share: number, digits = 1): string {
  return `${(share * 100).toFixed(digits)}%`;
}

export function formatDayShort(day: string): string {
  const [year, month, dayOfMonth] = day.split("-").map((part) => Number(part));
  if (year === undefined || month === undefined || dayOfMonth === undefined) return day;
  const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  return `${months[month - 1] ?? ""} ${dayOfMonth}`;
}

/** "Jul 15 to Aug 13" style window label. */
export function formatDayRange(sinceDay: string, untilDay: string): string {
  if (!sinceDay || !untilDay) return "";
  if (sinceDay === untilDay) return formatDayShort(sinceDay);
  return `${formatDayShort(sinceDay)} to ${formatDayShort(untilDay)}`;
}

export function enumerateDays(sinceDay: string, untilDay: string): readonly string[] {
  const days: string[] = [];
  const start = Date.parse(`${sinceDay}T00:00:00Z`);
  const end = Date.parse(`${untilDay}T00:00:00Z`);
  if (Number.isNaN(start) || Number.isNaN(end) || end < start) return days;
  for (let cursor = start; cursor <= end; cursor += 86_400_000) {
    days.push(new Date(cursor).toISOString().slice(0, 10));
  }
  return days;
}

export function enumerateHourStarts(sinceTime: string, untilTime: string): readonly string[] {
  const starts: string[] = [];
  const start = Date.parse(sinceTime);
  const end = Date.parse(untilTime);
  if (Number.isNaN(start) || Number.isNaN(end) || end <= start) return starts;
  for (let cursor = start; cursor < end; cursor += HOUR_MS) {
    starts.push(new Date(cursor).toISOString());
  }
  return starts;
}

export type UsageWindow = {
  sinceDay: string;
  untilDay: string;
  timeZone: string;
  resolution: "day" | "hour";
  sinceTime?: string;
  untilTime?: string;
};

/** Calendar / rolling window in the viewer's zone (T3 makeWindow port). */
export function makeWindow(days: number, now = new Date()): UsageWindow {
  const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  const format = new Intl.DateTimeFormat("en-CA", {
    timeZone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  });
  if (days === 1) {
    const untilTimeMs = Math.floor(now.getTime() / 60_000) * 60_000;
    const sinceTimeMs = untilTimeMs - 24 * HOUR_MS;
    const sinceTime = new Date(sinceTimeMs);
    const untilTime = new Date(untilTimeMs);
    return {
      sinceDay: format.format(sinceTime),
      untilDay: format.format(untilTime),
      timeZone,
      resolution: "hour",
      sinceTime: sinceTime.toISOString(),
      untilTime: untilTime.toISOString(),
    };
  }
  const untilDay = format.format(now);
  const [year = 0, month = 1, dayOfMonth = 1] = untilDay.split("-").map((part) => Number.parseInt(part, 10));
  const start = new Date(Date.UTC(year, month - 1, dayOfMonth - (days - 1)));
  return {
    sinceDay: start.toISOString().slice(0, 10),
    untilDay,
    timeZone,
    resolution: "day",
  };
}
