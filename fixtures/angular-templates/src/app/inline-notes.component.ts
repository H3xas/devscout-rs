import { Component } from '@angular/core';

@Component({
  selector: 'app-single-note',
  template: '<p>{{ firstNote }}</p>',
})
export class SingleNote {
  firstNote = '';
}

@Component({
  selector: 'app-double-note',
  template: "<p [title]=\"secondNote\">note</p>",
})
export class DoubleNote {
  secondNote = '';
}

@Component({
  selector: 'app-backtick-note',
  template: `
    <section>
      <h2>{{ thirdNote }}</h2>
      <button (click)="dismiss()">close</button>
    </section>
  `,
})
export class BacktickNote {
  thirdNote = '';

  dismiss(): void {}
}
