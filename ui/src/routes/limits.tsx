import { Limits } from "@/features/limits/Limits";
import { useAgentSession } from "@/features/session/SessionProvider";

export function LimitsRoute() {
  const session = useAgentSession();
  return <Limits pollMinutes={session.appSettings.limitsPollMinutes} />;
}
