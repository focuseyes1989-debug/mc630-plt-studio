use super::*;
use crate::shape_ops::Target;
fn fixture()->A4Studio {
    let mut app=A4Studio::fresh_tab();
    app.layout.design_shapes.push(sheet::DesignShape {
        path:vec![[20.,20.],[30.,20.],[30.,30.],[20.,20.]],
        fill:Some([20,80,140,255]),stroke:[0,0,0,255],stroke_width:0.3,print:true,cut:true,
    });
    app.layout.manual_cut_paths.push(vec![[40.,20.],[50.,20.]]);
    app
}
#[test]
fn arrow_keys_move_designs_with_all_modifier_combinations() {
    for (shift,alt,ctrl,step) in [(false,false,false,0.1),(true,false,false,1.),(false,true,false,0.01),
        (false,false,true,1.),(true,false,true,10.),(true,true,false,0.1),(false,true,true,0.1),(true,true,true,1.)] {
        let ctx=egui::Context::default();let mut app=fixture();app.selected_design=Some(0);
        let mods=egui::Modifiers {shift,alt,ctrl,command:ctrl,..Default::default()};
        let frame=|app:&mut A4Studio,events|{let _=ctx.run(egui::RawInput {
            screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1280.,900.))),
            modifiers:mods,events,..Default::default()
        },|ctx|app.ui(ctx));};
        frame(&mut app,vec![]);
        frame(&mut app,vec![egui::Event::Key {key:egui::Key::ArrowRight,physical_key:None,pressed:true,repeat:false,modifiers:mods}]);
        assert!((app.layout.design_shapes[0].path[0][0]-20.-step).abs()<1e-8,"{mods:?}");
        assert_eq!(app.layout.manual_cut_paths[0][0],[40.,20.]);
        assert_eq!(app.undo.len(),1);
    }
}
#[test]
fn design_cutline_and_image_nudge_together_and_undo_in_one_step() {
    let ctx=egui::Context::default();let mut app=fixture();
    app.layout.count=1;app.layout.custom.insert(0,sheet::Placement{x:60.,y:40.,width:10.,height:10.,orientation:Default::default()});
    app.selection.insert(0);app.shape_selection=vec![Target::Design(0),Target::Cut(0),Target::Cut(0)];
    let before=app.layout.clone();app.nudge_canvas_selection(1.,-0.1);
    assert_eq!(app.layout.design_shapes[0].path[0],[21.,19.9]);
    assert_eq!(app.layout.manual_cut_paths[0][0],[41.,19.9]);
    assert_eq!(app.layout.custom[&0].x,61.);assert_eq!(app.layout.custom[&0].y,39.9);
    assert_eq!(app.layout.design_shapes[0].fill,before.design_shapes[0].fill);
    assert_eq!(app.undo.len(),1);let moved=app.layout.clone();
    app.undo_object(&ctx);assert!(app.layout==before);
    app.redo_object(&ctx);assert!(app.layout==moved);
}
#[test]
fn single_cutline_nudge_clamps_at_page_edges_without_empty_undo_steps() {
    let mut app=fixture();app.selected_cut_shape=Some(0);
    app.nudge_canvas_selection(-1000.,-1000.);
    assert_eq!(app.layout.manual_cut_paths[0],vec![[0.,0.],[10.,0.]]);
    let history=app.undo.len();app.nudge_canvas_selection(-1.,-1.);assert_eq!(app.undo.len(),history);
    let paths=app.layout.manual_cut_paths.clone();
    app.cut_draft=Some(Draft::new(Tool::Pen));
    app.nudge_canvas_selection(1.,1.);assert_eq!(app.layout.manual_cut_paths,paths);
}
#[test]
fn focused_text_field_keeps_arrow_navigation_without_moving_shapes() {
    let ctx=egui::Context::default();let mut app=fixture();app.selected_design=Some(0);
    let field=egui::Id::new("nudge-text-focus");
    let _=ctx.run(Default::default(),|ctx|ctx.memory_mut(|m|m.request_focus(field)));
    // A focused numeric/text editor's keyboard request is honored by the
    // shortcut gate; exercise it with an actual egui TextEdit before app UI.
    let mut text="12".to_string();
    let _=ctx.run(egui::RawInput {events:vec![egui::Event::Key {key:egui::Key::ArrowLeft,physical_key:None,pressed:true,repeat:false,modifiers:Default::default()}],..Default::default()},|ctx|{
        egui::TopBottomPanel::top("test-editor").show(ctx,|ui|{ui.add(egui::TextEdit::singleline(&mut text).id(field));});
        app.ui(ctx);
    });
    assert_eq!(app.layout.design_shapes[0].path[0],[20.,20.]);assert!(app.undo.is_empty());
}
