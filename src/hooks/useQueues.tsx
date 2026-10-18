import { createContext, useCallback, useContext, useEffect, useRef, useState } from 'react';
import type { QueueInfo } from '../types/download';
import { api } from '../services/api';

/** The queues, kept fresh: after changes, after downloads change state, and every so often (schedules open and close with time). */
export function useQueues() {
  const [queues, setQueues] = useState<QueueInfo[]>([]);
  const timer = useRef<number | undefined>(undefined);

  const refresh = useCallback(() => {
    api
      .getQueues()
      .then(setQueues)
      .catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
    const soon = () => {
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(refresh, 300);
    };
    const interval = window.setInterval(refresh, 15000);
    const unlisten = [api.onQueuesChanged(soon), api.onStatusChanged(soon)];
    return () => {
      window.clearInterval(interval);
      window.clearTimeout(timer.current);
      unlisten.forEach((p) => p.then((u) => u()).catch(() => {}));
    };
  }, [refresh]);

  return { queues, refresh };
}

/** Lets rows know whether their queue is waiting for its schedule, without threading props through the list. */
export const QueuesContext = createContext<QueueInfo[]>([]);
export const useQueueInfos = () => useContext(QueuesContext);

const DAY_NAMES = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

/** "02:00" today, "Tue 02:00" this week, otherwise with the date. */
export function whenText(unixSeconds: number): string {
  const d = new Date(unixSeconds * 1000);
  const time = d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  if (sameDay) return time;
  const tomorrow = new Date(now.getTime() + 86400000);
  if (d.toDateString() === tomorrow.toDateString()) return `tomorrow ${time}`;
  const days = (d.getTime() - now.getTime()) / 86400000;
  if (days < 7) return `${DAY_NAMES[d.getDay()]} ${time}`;
  return `${d.toLocaleDateString([], { month: 'short', day: 'numeric' })} ${time}`;
}
