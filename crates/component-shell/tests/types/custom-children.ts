import {
  div,
  type Element,
  type ElementChild,
  type Entity,
  type NativeElement,
} from 'gpui-kit';
import {
  InputState,
  ScrollbarHandle,
  Spinner,
  type SpinnerElement,
} from 'gpui-component';

declare const retainedView: Entity;

const spinner: SpinnerElement = new Spinner()
  .size('medium')
  .child('Loading')
  .p(2)
  .size('small');
const general: Element = spinner;
const children: ElementChild[] = [spinner, retainedView, 'label', 1, true];
const native: NativeElement = div().children(children);
const calledInput: InputState = InputState('Search', 'Ada');
const constructedInput: InputState = new InputState();
const calledScroll: ScrollbarHandle = ScrollbarHandle();
const constructedScroll: ScrollbarHandle = new ScrollbarHandle();

void [general, native, calledInput, constructedInput, calledScroll, constructedScroll];
