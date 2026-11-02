import React, { useEffect, useState } from 'react';
import { Link2 } from 'lucide-react';
import type { DownloadTask } from '../../types/download';
import { api } from '../../services/api';
import { formatBytes } from '../../utils/format';
import { isHttpUrl } from '../../lib/meta';
import { Dialog } from '../ui/Dialog';
import { Button } from '../ui/Button';
import { Input } from '../ui/Field';
import { errText } from '../../hooks/useDownloads';

/** Give an unfinished download a fresh link (an expired file-host link, a mirror) and resume it. */
export const RefreshLinkDialog: React.FC<{
  task: DownloadTask | null;
  onClose: () => void;
  onDone: (task: DownloadTask) => void;
}> = ({ task, onClose, onDone }) => {
  const [url, setUrl] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (task) {
      setUrl('');
      setBusy(false);
      setError(null);
    }
  }, [task]);

  const submit = async (e?: React.FormEvent) => {
    e?.preventDefault();
    if (!task || !isHttpUrl(url.trim()) || busy) return;
    setBusy(true);
    setError(null);
    try {
      await api.refreshLink(task.id, url.trim());
      onDone(task);
      onClose();
    } catch (err) {
      setError(errText(err));
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={!!task}
      onClose={onClose}
      width={500}
      title="Refresh link"
      description={
        task ? (
          <>
            Paste a new link for <span className="font-medium break-all text-fg">{task.filename}</span>
            {task.downloaded_bytes > 0 ? ` — it continues from ${formatBytes(task.downloaded_bytes)}.` : '.'}
          </>
        ) : null
      }
      footer={
        <>
          {error ? (
            <span className="mr-auto truncate text-[12px] text-danger" title={error}>
              {error}
            </span>
          ) : (
            <span className="mr-auto text-[12px] text-fg-3">It must be the same file (same size).</span>
          )}
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" loading={busy} disabled={!isHttpUrl(url.trim())} onClick={() => submit()}>
            Use this link
          </Button>
        </>
      }
    >
      <form onSubmit={submit}>
        <Input data-autofocus icon={Link2} value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://…" spellCheck={false} />
      </form>
    </Dialog>
  );
};
