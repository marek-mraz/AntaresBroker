from os.path import dirname, join, abspath
from os import walk
from pprint import PrettyPrinter


def get_list_files(root_dir: str) -> list:
    # Recursively traverse the directory structure
    my_list = list()
    for root, dirs, files in walk(root_dir):
        for file in files:
            if file.endswith('.resource'):
                my_list.append(join(root, file))

        for new_folder in dirs:
            new_folder = join(root_dir, new_folder)
            _ = get_list_files(root_dir=new_folder)

    return my_list


def find_index(lst, item):
    try:
        index = lst.index(item)
        return index
    except ValueError:
        return -1


def generate_data_original_apiutils(data: list):
    search_variables = '*** Variables ***\n'
    search_keywords = '*** Keywords ***\n'

    index_variables = find_index(data, search_variables)
    index_keywords = find_index(data, search_keywords)

    v = data[index_variables + 1:index_keywords]
    k = data[index_keywords + 1:]

    v = list(filter(lambda x: x != '\n', v))
    k = list(filter(lambda x: not x.startswith('    '), k))
    k = list(filter(lambda x: x != '\n', k))

    return v, k


def find_duplicates(lst: list):
    duplicates = set()
    unique_elements = set()

    for item in lst:
        if item in unique_elements:
            duplicates.add(item)
        else:
            unique_elements.add(item)

    return list(duplicates)


def read_resource_file(filename: str):
    with open(filename) as f:
        contents = f.readlines()

    variables, keywords = generate_data_original_apiutils(data=contents)

    variables = list(set(transform_spaces(lst=variables)))

    d_v = find_duplicates(variables)
    d_k = find_duplicates(keywords)

    if len(d_v) != 0:
        print(f'Duplicates: {d_v}')

    if len(d_k) != 0:
        print(f'Duplicates: {d_k}')

    return variables, keywords


def transform_spaces(lst):
    transformed_list = []
    for item in lst:
        transformed_item = ' '.join(item.split())
        transformed_list.append(transformed_item)
    return transformed_list


def read_new_resource_files(folder: str):
    file_list = get_list_files(root_dir=folder)

    v = list()
    k = list()
    for file in file_list:
        aux_v, aux_k = read_resource_file(filename=file)
        v.append(aux_v)
        k.append(aux_k)

    v = list(set([item for sublist in v for item in sublist]))
    v = list(set(transform_spaces(lst=v)))

    k = list(set([item for sublist in k for item in sublist]))

    return v, k


if __name__ == '__main__':
    # Get the folder of the ApiUtils.resources
    base_folder = dirname(dirname(abspath(__file__)))
    root_folder = base_folder + '/resources/ApiUtils'

    # Extract the variables and keywords from all new ApiUtils.resource
    new_v, new_k = (
        read_new_resource_files(folder=root_folder))

    # Show the information
    pp = PrettyPrinter(indent=4)

    print('\n\n\nList of Variables: ')
    pp.pprint(new_v)
    print('\n\n\n')
    print('List of Keywords: ')
    pp.pprint(new_k)
    print('\n\n\n')
