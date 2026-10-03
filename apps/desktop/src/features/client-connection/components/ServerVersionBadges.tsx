import { Badge } from "@/components/ui/badge";
import { checkVersionDrift } from "@/lib/version";

interface ServerVersionBadgesProps {
  version?: string | null;
}

export function ServerVersionBadges({ version }: ServerVersionBadgesProps) {
  const drift = checkVersionDrift(version);

  return (
    <>
      {version ? (
        <Badge variant="outline" className="text-[10px]">
          v{version}
        </Badge>
      ) : null}
      {drift.hasDrift ? (
        <Badge variant="warning" className="text-[10px] version-drift-badge" role="status">
          {drift.message ?? "Version drift"}
        </Badge>
      ) : null}
    </>
  );
}
