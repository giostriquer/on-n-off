import type { SignInPresentation } from "./loginState";
import { TermBadge } from "./TermBadge";

export function SignInBadge({ signIn }: { signIn: SignInPresentation | null }) {
  if (!signIn) return null;
  const tooltip = <>
    <div>{signIn.headline}</div>
    {signIn.countdown && <div>{signIn.countdown}</div>}
    <div>{signIn.next}</div>
  </>;
  return <TermBadge badge={signIn} tooltip={tooltip} />;
}
