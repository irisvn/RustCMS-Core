//! Quiz questions of a lesson. One form edits the
//! question and its rows: a choice question's options (one table row each),
//! or a fill-in-the-blank question's blanks. Switching a question to the
//! other kind removes the rows its form no longer shows, when saved.
//! Administrators only (instructors get no `quiz_*` permission).

use rustcms::admin::*;

use crate::models::{quiz_blank, quiz_option, quiz_question};

pub struct QuizQuestionResource;

fn kinds() -> Vec<(&'static str, &'static str)> {
    vec![
        (quiz_question::SINGLE, "One answer"),
        (quiz_question::MULTIPLE, "Several answers"),
        (quiz_question::FILL_BLANK, "Fill in the blanks"),
    ]
}

impl Resource for QuizQuestionResource {
    type Entity = quiz_question::Entity;
    const SLUG: &'static str = "quiz_questions";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Quiz question").title("prompt").icon("circle-help").group("Learning")
    }

    fn fields() -> Vec<Field> {
        use quiz_question::Column as C;
        vec![
            Field::column(C::LessonId).belongs_to("lessons").required().filterable(),
            Field::column(C::Kind).required().filterable(),
            Field::column(C::Prompt).required().searchable().max_length(1000),
            Field::column(C::Explain),
        ]
    }

    // #region quiz-form
    fn form() -> Form {
        Form::new().schema(components![
            SelectInput::make("lesson_id"),
            SelectInput::make("kind").options(kinds()),
            Textarea::make("prompt"),
            Toggle::make("explain").label("Hints on correct options"),
            // Options of a choice question: the options relation holds the rows
            // without a blank (`where_null`), each with an image and an
            // explanation (a compact rich editor: rows are short).
            Repeater::make("options")
                .relationship()
                .shown_when(when("kind").is_in([quiz_question::SINGLE, quiz_question::MULTIPLE]))
                .order_column("position")
                .schema(components![
                    TextInput::make("content"),
                    Toggle::make("correct"),
                    // Reads the question's form (`when_root`).
                    TextInput::make("hint")
                        .shown_when(Condition::all([when("correct").is_true(), when_root("explain").is_true()])),
                    FileUpload::make("image"),
                    RichEditor::make("explanation"),
                ])
                .add_action_label("Add an option"),
            Repeater::make("blanks")
                .relationship()
                .shown_when(when("kind").eq(quiz_question::FILL_BLANK))
                .order_column("position")
                .schema(components![TextInput::make("label")])
                .add_action_label("Add a blank"),
        ])
    }
    // #endregion quiz-form

    fn table() -> Table {
        Table::new()
            .columns(vec![
                TextColumn::make("prompt"),
                BadgeColumn::make("kind").options(kinds()),
                TextColumn::make("lesson_id"),
                TextColumn::make("options_count").label("Options").counts("options"),
            ])
            .filters(vec![SelectFilter::make("lesson_id"), SelectFilter::make("kind").options(kinds())])
    }

    // #region quiz-relations
    fn relations() -> Vec<Relation> {
        vec![
            Relation::has_many("options", "quiz_options", "quiz_question_id").where_null("blank_id"),
            Relation::has_many("blanks", "quiz_blanks", "quiz_question_id"),
        ]
    }
    // #endregion quiz-relations
}

pub struct QuizOptionResource;

impl Resource for QuizOptionResource {
    type Entity = quiz_option::Entity;
    const SLUG: &'static str = "quiz_options";

    /// Written with their question (it is locked first), found through it.
    fn meta() -> ResourceMeta {
        ResourceMeta::new("Quiz option")
            .title("content")
            .navigation(false)
            .lock_parent("quiz_question_id", "quiz_questions")
    }

    fn fields() -> Vec<Field> {
        use quiz_option::Column as C;
        vec![
            Field::column(C::QuizQuestionId).belongs_to("quiz_questions").required(),
            // Set by the relation the option is created through.
            Field::column(C::BlankId).guarded().readable(|_| true),
            Field::column(C::Content).required().max_length(500),
            Field::column(C::Correct),
            Field::column(C::Hint).required().max_length(500),
            Field::column(C::Image).media().accept(&["image/*"]).collection("quiz-images"),
            Field::column(C::Explanation).rich_text().collection("quiz-images"),
            // Sortable: the question's page lists its options in order.
            Field::column(C::Position).guarded().readable(|_| true).sortable(),
        ]
    }
}

pub struct QuizBlankResource;

impl Resource for QuizBlankResource {
    type Entity = quiz_blank::Entity;
    const SLUG: &'static str = "quiz_blanks";

    fn meta() -> ResourceMeta {
        ResourceMeta::new("Quiz blank")
            .title("label")
            .navigation(false)
            .lock_parent("quiz_question_id", "quiz_questions")
    }

    fn fields() -> Vec<Field> {
        use quiz_blank::Column as C;
        vec![
            Field::column(C::QuizQuestionId).belongs_to("quiz_questions").required(),
            Field::column(C::Label).required().max_length(80),
            Field::column(C::Position).guarded().readable(|_| true).sortable(),
        ]
    }
}
