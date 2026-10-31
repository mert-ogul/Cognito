import { Button } from "@/components/ui/button";

type Props = {
  preparing: boolean;
  progress: string | null;
  onDownload: () => void;
};

export function ModelsDownloadBanner({ preparing, progress, onDownload }: Props) {
  return (
    <div className="flex flex-wrap items-center gap-2 border-b border-line px-3 py-2">
      <p className="text-muted">Checking models download once (~2.1 GB), then you can ask questions.</p>
      <Button type="button" variant="secondary" disabled={preparing} onClick={onDownload}>
        {preparing ? "Downloading…" : "Download"}
      </Button>
      {progress ? <p className="text-muted">{progress}</p> : null}
    </div>
  );
}
