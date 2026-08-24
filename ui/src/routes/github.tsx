import { useNavigate } from "@tanstack/react-router";
import { Github } from "@/features/github/Github";
import { useAgentSession } from "@/features/session/SessionProvider";

export function GithubRoute() {
  const session = useAgentSession();
  const navigate = useNavigate();
  return (
    <Github
      pollSeconds={session.appSettings.githubPollSeconds}
      onOpenSettings={() => void navigate({ to: "/settings" })}
    />
  );
}
