import { Component } from '@angular/core';

@Component({
  selector: 'app-search-list',
  templateUrl: './search-list.component.html',
})
export class SearchList {
  item = '';
  items: string[] = [];
  entry = '';
  position = 0;
  pending = false;
  job = '';
  searchBox = '';

  search(text: string, event: unknown): void {}
}
