import { HookList } from "@/features/catalog/HookList";
import { useAgentSession } from "@/features/session/SessionProvider";
import { filterHookList, providerReadsHooks } from "$lib/catalog";
import { copy } from "$lib/copy";

export function HooksRoute() {
  const session = useAgentSession();
  const tab = session.currentTab.dto ?? session.emptyTabDto();
  const reads = providerReadsHooks(session.currentAgent.id);
  return (
    <HookList
      tab={tab}
      hooks={reads ? filterHookList(tab, session.currentTab.filter) : []}
      filterQuery={session.currentTab.filter}
      unread={reads ? undefined : copy.hooksUnread(session.currentAgent.displayName)}
    />
  );
}
