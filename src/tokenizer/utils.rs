pub fn find_next(str: &str, char: char, start: usize) -> Option<usize> {
    str.get(start..)?.find(char).map(|index| index + start)
}
