import { useMemo } from 'react';

export interface ArticleCardProps {
  title: string;
  likeCount: number;
  onLike?: () => void;
}

export function ArticleCard({ title, likeCount, onLike }: ArticleCardProps) {
  const label = useMemo(() => `${likeCount} likes`, [likeCount]);
  return (
    <div className="article-card">
      <h3>{title}</h3>
      <span>{label}</span>
      <button onClick={onLike}>Like</button>
    </div>
  );
}
