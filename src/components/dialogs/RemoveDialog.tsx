import React, { useEffect, useState } from 'react';
import type { DownloadTask } from '../../types/download';
import { Dialog } from '../ui/Dialog';
import { Button } from '../ui/Button';
import { Checkbox } from '../ui/Field';
import { plural } from '../../utils/format';

export const RemoveDialog: React.FC<{
  tasks: DownloadTask[];
  onClose: () => void;
  onConfirm: (deleteFiles: boolean) => Promise<void>;
}> = ({ tasks, onClose, onConfirm }) => {
  const [deleteFiles, setDeleteFiles] = useState(false);
  const [busy, setBusy] = useState(false);
  const open = tasks.length > 0;

  useEffect(() => {
    if (open) {
      setDeleteFiles(false);
      setBusy(false);
    }
  }, [open]);

  const single = tasks.length === 1 ? tasks[0] : null;
  const active = tasks.filter((t) => t.status === 'downloading' || t.status === 'queued').length;

  return (
    <Dialog
      open={open}
      onClose={onClose}
      width={420}
      title={single ? 'Remove download?' : `Remove ${plural(tasks.length, 'download')}?`}
      description={
        single ? (
          <>
            <span className="font-medium break-all text-fg">{single.filename}</span> will be removed from the list.
          </>
        ) : (
          'They will be removed from the list.'
        )
      }
      footer={
        <>
          <span className="mr-auto" />
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="danger"
            data-autofocus
            loading={busy}
            onClick={async () => {
              setBusy(true);
              await onConfirm(deleteFiles);
              onClose();
            }}
          >
            {deleteFiles ? 'Remove and delete' : 'Remove'}
          </Button>
        </>
      }
    >
      <div className="space-y-3">
        {active > 0 && (
          <p className="text-[12.5px] text-warning">
            {active === tasks.length && single ? 'This download is in progress and will be stopped.' : `${plural(active, 'download')} in progress will be stopped.`}
          </p>
        )}
        <label className="flex cursor-default items-start gap-2.5 rounded-lg bg-surface-2 p-3 shadow-[inset_0_0_0_1px_var(--line)]">
          <Checkbox checked={deleteFiles} onChange={setDeleteFiles} className="mt-0.5" label="Also delete files" />
          <span onClick={() => setDeleteFiles((v) => !v)}>
            <span className="block text-[13px] font-medium text-fg">Also delete {single ? 'the file' : 'the files'} from disk</span>
            <span className="block text-[12px] text-fg-3">Includes any partially downloaded data. This can’t be undone.</span>
          </span>
        </label>
      </div>
    </Dialog>
  );
};
