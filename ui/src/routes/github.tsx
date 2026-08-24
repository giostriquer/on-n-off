import { useNavigate } from "@tanstack/react-router";
import { Github } from "@/features/github/Github";
import { SCREEN_PATH, useAgentSession } from "@/features/session/SessionProvider";

export function GithubRoute() {
  const session = useAgentSession();
  const navigate = useNavigate();
  return (
    <Github
      pollSeconds={session.appSettings.githubPollSeconds}
      onOpenSettings={() => void navigate({ to: SCREEN_PATH.settings })}
    />
  );
}
