import type { DesktopPresentation } from "./desktopAccount";
import { TermBadge } from "./TermBadge";

export function DesktopAccountBadge({ desktop }: { desktop: DesktopPresentation }) {
  return <TermBadge badge={desktop} tooltip={desktop.headline} />;
}
