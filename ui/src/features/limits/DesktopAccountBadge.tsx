import type { DesktopPresentation } from "./desktopAccount";
import { TermBadge } from "./TermBadge";

export function DesktopAccountBadge({ desktop }: { desktop: DesktopPresentation }) {
  return <TermBadge badge={desktop} tooltip={<>
    <div>{desktop.headline}</div>
    <div>{desktop.next}</div>
  </>} />;
}
