import { HookList } from "@/features/catalog/HookList";
import { useAgentSession } from "@/features/session/SessionProvider";
import { copy } from "$lib/copy";

export function HooksRoute() {
  const session = useAgentSession();
  const reads = session.currentAgent.readsHooks;
  return (
    <HookList
      tab={session.currentTab.dto ?? session.emptyTabDto()}
      hooks={reads ? (session.filtered?.hooks ?? []) : []}
      filterQuery={session.currentTab.filter}
      unread={reads ? undefined : copy.hooksUnread(session.currentAgent.displayName)}
    />
  );
}
