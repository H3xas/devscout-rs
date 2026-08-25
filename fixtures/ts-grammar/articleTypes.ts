export interface ArticleItem {
  id: string;
  createdAt: string;
  author: ArticleAuthor;
}

export interface ArticleAuthor {
  id: string;
  displayName: string;
}

export type ArticleItemStatus = 'draft' | 'published' | 'archived';

export type ArticlePage = {
  items: ArticleItem[];
  nextCursor: string | null;
};
