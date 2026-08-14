type AgentBannerProps = {
  message: string;
};

export function AgentBanner({ message }: AgentBannerProps) {
  return (
    <div className="bg-[var(--trip)] px-4 py-2 text-[13px] text-[#f7f1ea]" role="alert">
      {message}
    </div>
  );
}
