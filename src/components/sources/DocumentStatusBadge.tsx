import { Badge } from "@/components/ui/badge";

type Props = { status: string; errorMessage: string | null };

export function DocumentStatusBadge({ status, errorMessage }: Props) {
  const s = status.toLowerCase();
  if (s === "indexed" || s === "completed") {
    return <Badge variant="success">indexed</Badge>;
  }
  if (s === "failed" || errorMessage) {
    return <Badge variant="danger">failed</Badge>;
  }
  if (s === "pending" || s === "processing" || s === "importing") {
    return <Badge variant="warning">{status}</Badge>;
  }
  return <Badge variant="muted">{status}</Badge>;
}
