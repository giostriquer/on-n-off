import { useNavigate } from "@tanstack/react-router";
import { Github } from "@/features/github/Github";
import { SCREEN_PATH } from "@/features/session/SessionProvider";
import { useAppSettings } from "$lib/useAppSettings";

export function GithubRoute() {
  const { settings } = useAppSettings();
  const navigate = useNavigate();
  return (
    <Github
      pollSeconds={settings.githubPollSeconds}
      onOpenSettings={() => void navigate({ to: SCREEN_PATH.settings })}
    />
  );
}
