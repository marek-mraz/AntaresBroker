import re


class Requests:
    def __init__(self, variables, apiutils_variables, config_file, template_params_value, test_name, name):
        self.op = {
            'Create Entity': {
                'positions': [0],
                'params': ['filename']
            },
            'Create Entity Selecting Content Type': {
                'positions': [0, 2],
                'params': ['filename', 'content_type']
            },
            'Create Subscription': {
                'positions': [1, 2],
                'params': ['filename', 'content_type']
            },
            'Create Or Update Temporal Representation Of Entity Selecting Content Type':  {
                'positions': [],
                'params': ['temporal_entity_representation_id', 'filename', 'content_type', 'accept']
            },
            'Create Entity From File': {
                'positions': [0],
                'params': ['filename']
            },
            'Call Api Endpoint With Invalid Parameter': {
                'positions': [0, 1, 2, 3],
                'params': ['method', 'endpoint', 'filename', 'extraParam']
            },
            'Create Entity From JSON-LD Content': {
                'positions': [0],
                'params': ['content']
            },
            'Batch Create Entities': {
                'positions': [1],
                'params': ['content_type']
            },
            'Create Context Source Registration With Return': {
                'positions': [0],
                'params': ['filename']
            },
            'Retrieve Entity': {
                'positions': [],
                'params': ['id', 'accept', 'attrs', 'context', 'geometryProperty', 'options', 'format', 'lang',
                           'join', 'joinLevel', 'pick', 'omit', 'local']
            },
            'Retrieve Subscription': {
                'positions': [],
                'params': ['id', 'accept', 'context', 'content_type']
            },
            'Query Context Source Registrations With Return': {
                'positions': [0, 1],
                'params': ["type", "accept"]
            },
            'Query Temporal Representation Of Entities With Return': {
                'positions': [],
                'params': []
            },
            'Partial Update Entity Attributes': {
                'positions': [],
                'params': ['entityId', 'attributeId', 'fragment_filename', 'content_type', 'accept', 'context']
            },
            'Update Subscription': {
                'positions': [1, 2, 3],
                'params': ['filename', 'content', 'context']

            },
            'Query Context Source Registration Subscriptions': {
                'positions': [],
                'params': ['context', 'limit', 'page', 'accept']
            },
            'Query Temporal Representation Of Entities': {
                'positions': [],
                'params': ['context', 'entity_types', 'entity_ids', 'entity_id_pattern',
                           'ngsild_query', 'csf', 'georel', 'geometry',
                           'coordinates', 'geoproperty', 'timerel', 'timeAt', 'endTimeAt',
                           'attrs', 'limit', 'lastN', 'accept', 'options', 'format', 'datasetId',
                           'aggrMethods', 'aggrPeriodDuration', 'pick', 'omit', 'local']
            },
            'Query Temporal Representation Of Entities Via Post': {
                'positions': [],
                'params': ['query_file_name', 'content_type', 'context']
            },
            'Retrieve Attribute': {
                'positions': [0],
                'params': ['attribute_name']
            },
            'Retrieve Entity Type': {
                'positions': [0, 1],
                'params': ['type', 'context']
            },
            'Query Entities': {
                'positions': [],
                'params': ['entity_ids', 'entity_types', 'accept',
                           'attrs', 'context', 'geoproperty',
                           'options', 'limit', 'entity_id_pattern',
                           'scopeq', 'georel', 'coordinates', 'geometry', 'count', 'q', 'datasetId',
                           'join', 'joinLevel', 'pick', 'omit', 'orderBy', 'local']
            },
            'Query Entities Via POST': {
                'positions': [],
                'params': ['entities', 'content_type', 'accept',
                           'context', 'attrs', 'geometry_property', 'join', 'joinLevel', 'options', 'local']
            },
            'Retrieve Temporal Representation Of Entity': {
                'positions': [],
                'params': ['temporal_entity_representation_id', 'attrs', 'options',
                           'context', 'timerel', 'timeAt', 'endTimeAt', 'lastN', 'accept',
                           'aggrMethods', 'aggrPeriodDuration', 'timeproperty', 'pick', 'omit']
            },
            'Delete Entity': {
                'positions': [0],
                'params': ['id']
            },
            'Replace Entity': {
                'positions': [0, 1, 2, 3],
                'params': ['entity_id', 'filename', 'content_type', 'context']
            },
            'Replace Entity Selecting Content Type': {
                'positions': [0, 1, 2, 3],
                'params': ['entity_id', 'entity_fragment', 'content_type', 'context']
            },
            'Purge Entities': {
                'positions': [],
                'params': ['type', 'id', 'q', 'keep', 'drop', 'context', 'local']
            },
            'Merge Entity': {
                'positions': [0, 1, 2, 3],
                'params': ['entity_id', 'entity_filename', 'content_type', 'context']
            },
            'Replace Attribute Selecting Content Type': {
                'positions': [0, 1, 2, 3, 4],
                'params': ['entity_id', 'attr_id', 'attribute_fragment', 'content_type', 'context']
            },
            'Append Entity Attributes': {
                'positions': [0, 1, 2],
                'params': ['id', 'fragment_filename', 'content_type']
            },
            'Update Entity Attributes': {
                'positions': [0, 1, 2],
                'params': ['id', 'fragment_filename', 'content_type']
            },
            'Delete Temporal Representation Of Entity With Returning Response': {
                'positions': [0],
                'params': ['id']
            },
            'Append Attribute To Temporal Entity': {
                'positions': [0, 1, 2],
                'params': ['id', 'fragment_filename', 'content_type']
            },
            'Delete Subscription': {
                'positions': [0],
                'params': ['id']
            },
            'Query Subscriptions': {
                'positions': [],
                'params': ['context', 'limit', 'offset', 'accept']
            },
            'Retrieve Context Source Registration Subscription': {
                'positions': [],
                'params': ['subscription_id', 'context', 'accept']
            },
            'Retrieve Context Source Registration': {
                'positions': [],
                'params': ['context_source_registration_id', 'context', 'accept']
            },
            'Delete Context Source Registration With Return': {
                'positions': [0],
                'params': ['id']
            },
            'Query Context Source Registrations': {
                'positions': [],
                'params': ['context', 'id', 'type', 'idPattern', 'attrs',
                           'q', 'csf', 'georel', 'geometry',
                           'coordinates', 'geoproperty', 'timeproperty', 'timerel',
                           'timeAt', 'limit', 'offset', 'accept']
            },
            'Update Context Source Registration With Return': {
                'positions': [0, 1, 2],
                'params': ['id', 'filename', 'content']
            },
            'Retrieve context source registration subscription': {
                'positions': [0],
                'params': ['id']
            },
            'Create Context Source Registration Subscription': {
                'positions': [0],
                'params': ['filename']
            },
            'Create Context Source Registration': {
                'positions': [0],
                'params': ['context_source_registration_payload']
            },
            'Delete Context Source Registration Subscription': {
                'positions': [0],
                'params': ['id']
            },
            'Update Context Source Registration Subscription': {
                'positions': [0, 1],
                'params': ['subscription_id', 'subscription_update_fragment']
            },
            'Update Context Source Registration': {
                'positions': [0, 1],
                'params': ['context_source_registration_id', 'update_fragment']
            },
            'Update Context Source Registration From File': {
                'positions': [0, 1],
                'params': ['context_source_registration_id', 'filename']
            },
            'Update Context Source Registration Subscription From File': {
                'positions': [0, 1],
                'params': ['subscription_id', 'subscription_update_fragment']
            },
            'Retrieve Attributes': {
                'positions': [],
                'params': ['context', 'details', 'accept']
            },
            'Retrieve Entity Types': {
                'positions': [],
                'params': ['context', 'details', 'accept']
            },
            'Batch Request Entities From File': {
                'positions': [0, 1],
                'params': ['operation', 'filename']
            },
            'Batch Delete Entities': {
                'positions': [],
                'params': ['entities_ids_to_be_deleted', 'teardown']
            },
            'Batch Update Entities': {
                'positions': [0],
                'params': ['entities', 'overwrite_option']
            },
            'Batch Upsert Entities': {
                'positions': [],
                'params': ['entities_to_be_upserted', 'update_option']
            },
            'Batch Merge Entities': {
                'positions': [],
                'params': ['entities_to_be_merged']
            },
            'Request Entity From File': {
                'positions': [0],
                'params': ['filename']
            },
            'Delete Entity Attributes': {
                'positions': [],
                'params': ['entityId', 'attributeId', 'datasetId', 'deleteAll', 'context']
            },
            'Create Temporal Representation Of Entity Selecting Content Type': {
                'positions': [0, 1],
                'params': ['filename', 'content_type']
            },
            'Delete Attribute From Temporal Entity': {
                'positions': [],
                'params': ['entityId', 'attributeId', 'content_type', 'datasetId', 'deleteAll', 'context']
            },
            'Delete Attribute Instance From Temporal Entity': {
                'positions': [0, 1, 2, 3, 4],
                'params': ['temporal_entity_id', 'attributeId', 'instanceId', 'content_type', 'context']
            },
            'Modify Attribute Instance From Temporal Entity': {
                'positions': [0, 1, 2, 3, 4],
                'params': ['temporal_entity_id', 'attributeId', 'instanceId', 'fragment_filename', 'content_type', 'context']
            },
            'Create Subscription From File': {
                'positions': [0],
                'params': ['filename']
            },
            'Wait for notification': {
                'positions': [0],
                'params': ['timeout']
            },
            'Append Entity Attributes With Parameters': {
                'positions': [0, 1, 2, 3],
                'params': ['id', 'fragment_filename', 'content_type', 'options']
            },
            'Setup Initial Subscriptions': {
                'positions': [],
                'params': []
            },
            'List @contexts': {
                'positions': [0, 1],
                'params': ['details', 'kind']

            },
            'Serve a @context': {
                'positions': [0, 1],
                'params': ['contextId', 'details']

            },
            'Add a new @context': {
                'positions': [0],
                'params': ['filename']
            },
            'Delete a @context': {
                'positions': [0],
                'params': ['uri']
            }
        }

        self.description = {
            'Create Entity':
                Requests.create_entity,
            'Create Entity Selecting Content Type':
                Requests.create_entity_selecting_content_type,
            'Create Subscription':
                Requests.create_entity_selecting_content_type,
            'Create Entity From File':
                Requests.create_entity_from_file,
            'Call Api Endpoint With Invalid Parameter':
                Requests.call_api_endpoint_with_invalid_parameter,
            'Create Or Update Temporal Representation Of Entity Selecting Content Type':
                Requests.create_or_update_temporal_representation_of_entity_selecting_content_type,
            'Batch Create Entities':
                Requests.batch_create_entities,
            'Create Context Source Registration With Return':
                Requests.create_context_source_registration_with_return,
            'Retrieve Entity':
                Requests.retrieve_entity,
            'Retrieve Subscription':
                Requests.retrieve_subscription,
            'Query Context Source Registrations With Return':
                Requests.query_context_source_registrations_with_return,
            'Query Temporal Representation Of Entities With Return':
                Requests.query_temporal_representation_of_entities_with_return,
            'Partial Update Entity Attributes':
                Requests.partial_update_entity_attributes,
            'Update Subscription':
                Requests.update_subscription,
            'Query Context Source Registration Subscriptions':
                Requests.query_context_source_registration_subscriptions,
            'Query Temporal Representation Of Entities':
                Requests.query_temporal_representation_of_entities,
            'Append Entity Attributes With Parameters':
                Requests.append_entity_attributes_with_parameters,
            'Retrieve Attribute':
                Requests.retrieve_attribute,
            'Retrieve Entity Type':
                Requests.retrieve_entity_type,
            'Query Entities':
                Requests.query_entities,
            'Delete Entity':
                Requests.delete_entity_by_id,
            'Replace Entity':
                Requests.replace_entity,
            'Replace Entity Selecting Content Type':
                Requests.replace_entity_selecting_content_type,
            'Purge Entities':
                Request.purge_entities,
            'Merge Entity':
                Requests.merge_entity,
            'Replace Attribute Selecting Content Type':
                Requests.replace_attribute_selecting_content_type,
            'Append Entity Attributes':
                Requests.append_entity_attributes,
            'Update Entity Attributes':
                Requests.update_entity_attributes,
            'Retrieve Temporal Representation Of Entity':
                Requests.retrieve_temporal_representation_of_entity,
            'Delete Temporal Representation Of Entity With Returning Response':
                Requests.delete_temporal_representation_of_entity_with_returning_response,
            'Append Attribute To Temporal Entity':
                Requests.append_attribute_to_temporal_entity,
            'Delete Subscription':
                Requests.delete_subscription,
            'Query Subscriptions':
                Requests.query_subscriptions,
            'Retrieve Context Source Registration Subscription':
                Requests.retrieve_context_source_registration_subscription,
            'Retrieve Context Source Registration':
                Requests.retrieve_context_source_registration,
            'Delete Context Source Registration With Return':
                Requests.delete_context_source_registration_with_return,
            'Query Context Source Registrations':
                Requests.query_context_source_registrations,
            'Update Context Source Registration With Return':
                Requests.update_context_source_registration_with_return,
            'Update Context Source Registration':
                Requests.update_context_source_registration,
            'Update Context Source Registration From File':
                Requests.update_context_source_registration_from_file,
            'Retrieve context source registration subscription':
                Requests.retrieve_context_source_registration_subscription_2,
            'Create Context Source Registration Subscription':
                Requests.create_context_source_registration_subscription,
            'Delete Context Source Registration Subscription':
                Requests.delete_context_source_registration_subscription,
            'Update Context Source Registration Subscription':
                Requests.update_context_source_registration_subscription,
            'Update Context Source Registration Subscription From File':
                Requests.update_context_source_registration_subscription_from_file,
            'Retrieve Attributes':
                Requests.retrieve_attributes,
            'Retrieve Entity Types':
                Requests.retrieve_entity_types,
            'Batch Request Entities From File':
                Requests.batch_request_entities_from_file,
            'Batch Delete Entities':
                Requests.batch_delete_entities,
            'Batch Update Entities':
                Requests.batch_update_entities,
            'Batch Upsert Entities':
                Requests.batch_upsert_entities,
            'Batch Merge Entities':
                Requests.batch_merge_entities,
            'Request Entity From File':
                Requests.request_entity_from_file,
            'Delete Entity Attributes':
                Requests.delete_entity_attributes,
            'Create Temporal Representation Of Entity Selecting Content Type':
                Requests.create_temporal_representation_of_entity_selecting_content_type,
            'Delete Attribute From Temporal Entity':
                Requests.delete_attribute_from_temporal_entity,
            'Delete Attribute Instance From Temporal Entity':
                Requests.delete_attribute_instance_from_temporal_entity,
            'Modify Attribute Instance From Temporal Entity':
                Requests.modify_attribute_instance_from_temporal_entity,
            'Create Subscription From File':
                Requests.create_subscription_from_file,
            'Wait for notification':
                Requests.wait_for_notification,
            'Create Context Source Registration':
                Requests.create_context_source_registration,
            'Query Entities Via POST':
                Requests.query_entities_via_post,
            'Query Temporal Representation Of Entities Via Post':
                Requests.query_temporal_representation_of_entities_via_post,
            'Setup Initial Subscriptions':
                Requests.setup_initial_subscriptions,
            'List @contexts':
                Requests.list_contexts,
            'Serve a @context':
                Requests.serve_a_context,
            'Add a new @context':
                Requests.add_a_new_context,
            'Delete a @context':
                Requests.delete_a_context
        }

        self.variables = variables
        self.apiutils_variables = apiutils_variables
        self.config_file = config_file
        self.template_params_value = template_params_value
        self.test_name = test_name
        self.name = name

    def get_description(self, string):
        keys = self.op.keys()
        params = dict()

        # New version
        #lines_starting_response = re.findall(r'^\s*\$\{response\}.*|^\s*\$\{notification\}.*', string, re.MULTILINE)
        lines_starting_response = re.findall(r'^\s*\$\{response\}.*', string, re.MULTILINE)
        
        # Filter out lines that are Check operations (assertions), not request operations
        lines_starting_response = [line for line in lines_starting_response if 'Check Response Status Code' not in line and not re.search(r'\$\{response\}=\s+Check\s+', line)]
        
        # Also filter out empty or whitespace-only lines
        lines_starting_response = [line for line in lines_starting_response if line.strip() != '']

        if len(lines_starting_response) == 0:
            raise Exception(f"Error: No valid response lines found in test case")

        # If there is more than one line, it means that the test case has several operations, all of them to
        # create the environment content to execute the last one, which is the correct one to test the Test Case
        if len(lines_starting_response) > 1:
            # The last one corresponds to the execution of the test, the rest corresponds to the initial condition of
            # test case...
            response_to_check = lines_starting_response[-1]
        else:
            response_to_check = lines_starting_response[0]

        index = string.find(response_to_check)
        aux = string[index:].split('\n')
        aux = [x for x in aux if x.strip() != '']
        
        if len(aux) == 0 or aux[0].strip() == '':
            raise Exception(f"Error: No valid content found after response line. response_to_check='{response_to_check}'")

        params = list()
        request = str()

        # Get the list of params of the function, they are the keys
        if len(aux) == 1 or (len(aux) > 1 and '    ...    ' not in aux[1]):
            # the attributes are in the same line
            regex = r"\s*\$\{response\}=\s{4}(.*)"
            matches = re.finditer(regex, response_to_check, re.MULTILINE)
            request_parts = aux[0].split('    ')
            # Filter out empty strings and get the keyword (should be after ${response}=)
            non_empty_parts = [x for x in request_parts if x != '']
            if len(non_empty_parts) >= 2:
                request_full = non_empty_parts[1]
            else:
                raise Exception(f"Error: unexpected format in line, received: '{aux[0]}', expected format: '    ${{response}}=    Keyword Name    params...'")
            # Extract only the keyword name without parameters
            request = re.split(r'\s{2,}', request_full)[0]

            # We have two options from here, or the parameters are defined in the same line or the parameters are defined in
            # following lines, next lines
            for match in matches:
                # Check that we have 1 group matched
                if len(match.groups()) == 1:
                    aux = match.group(1)

                    # Get the list of keys
                    params = aux.split('    ')[1:]
                else:
                    raise Exception(f"Error: unexpected format, received: '{response_to_check}'")

            params = self.find_attributes_in_the_same_line(request_name=request, params=params)
        elif '    ...    ' in aux[1]:
            request_parts = aux[0].split('    ')
            request_full = [x for x in request_parts if x != ''][1]
            # Extract only the keyword name without parameters
            request = re.split(r'\s{2,}', request_full)[0]
            # We are in the case that the attributes are in following lines
            for i in range(1, len(aux)):
                if '    ...    ' in aux[i]:
                    regex = r'(\s{4})*\s{4}\.{3}\s{4}(.*)'
                    param = re.match(pattern=regex, string=aux[i])
                    if aux:
                        params.append(param.groups()[1])
                else:
                    break

            params = self.find_attributes_in_the_same_line(request_name=request, params=params)

        params = self.change_param_value(params)

        # Need to check if the key of the params is a variable (Cases 037_01, 037_03, 037_10)
        params = self.resolve_variable_key_in_params(params)

        description = self.description[request](params)
        return description

    def resolve_variable_key_in_params(self, params: dict) -> dict:
        new_dict = dict()

        for old_key, value in params.items():
            if '${' in old_key:
                new_key = self.change_param_value_iter(value=old_key)
                new_dict[new_key] = value
            else:
                new_dict[old_key] = value

        return new_dict

    def find_attributes_in_the_same_line(self, request_name, params):
        param = dict()
        if len(self.op[request_name]['positions']) == 0:
            # We do not know the position of the different parameters and the order in which they are received,
            # therefore in these cases all the parameters have identified the corresponding name
            # param = {x.split('=')[0]: self.get_value_simple(x.split('=')[1]) for x in params}
            for i in range(0, len(params)):
                x = params[i]
                if x.find('=') != -1:
                    aux = x.split('=')
                    key = aux[0]
                    value = self.get_value_simple(aux[1])

                    param[key] = value
                else:
                    key = self.op[request_name]['params'][i]
                    param[key] = x
        else:
            for i in range(0, len(self.op[request_name]['positions'])):
                param_position = self.op[request_name]['positions'][i]
                param_key = self.op[request_name]['params'][i]
                param_value = self.get_value(params=params, param_position=param_position, param_key=param_key)
                param[param_key] = param_value

        return param

    def find_attributes_next_lines(self, string, position, request_name):
        aux = string[position+len(request_name)+1:].split('\n')

        params = list()
        for a in range(0, len(aux)):
            param = aux[a]
            if param.startswith("    ..."):
                params.append(param.split('    ')[-1])
            else:
                break

        param = dict()
        for i in range(0, len(self.op[request_name]['positions'])):
            param_position = self.op[request_name]['positions'][i] - 1
            param_key = self.op[request_name]['params'][i]
            param_value = self.get_value(params=params, param_position=param_position, param_key=param_key)
            param[param_key] = param_value

        return param

    def change_param_value_iter(self, value):
        try:
            # Check if we can get the data from the current robot files
            result = self.variables[value]
        except KeyError:
            try:
                # Check if we can get the data from the apiutils file
                result = self.apiutils_variables[value]
            except KeyError:
                try:
                    aux = re.findall(pattern=r'\$\{(.*)}', string=value)
                    if len(aux) != 0:
                        aux = aux[0]
                    else:
                        aux = value
                    result = self.config_file.get_variable(aux)
                except KeyError:
                    try:
                        # aux = self.template_params_value[self.test_name]
                        aux = self.template_params_value[self.name]
                        result = aux[value]

                        if result[:2] == "${":
                            result = self.change_param_value_iter(result)
                    except KeyError:
                        result = value

        return result

    def change_param_value(self, position):
        for k, v in position.items():
            position[k] = self.change_param_value_iter(value=v)

        return position

    @staticmethod
    def create_entity(kwargs) -> str:
        if 'filename' in kwargs:
            result = (f"Request Header['Content-Type'] set to 'application/ld+json' and\n "
                      f"payload defined in file: '{kwargs['filename']}'")
            return result
        else:
            raise Exception(f"ERROR: expected filename, but received {kwargs}")

    @staticmethod
    def create_entity_selecting_content_type(kwargs) -> str:
        if 'filename' in kwargs and 'content_type' in kwargs:
            result = (f"Request Header['Content-Type'] set to '{kwargs['content_type']}' and\n "
                      f"payload defined in file: '{kwargs['filename']}'")
            return result
        else:
            raise Exception(f"ERROR: expected filename and content_type attributes, but received {kwargs}")

    @staticmethod
    def append_entity_attributes_with_parameters(kwargs) -> str:
        expected_parameters = ['id', 'fragment_filename', 'content_type', 'options']

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Append entity attributes with parameters:"
        for key, value in kwargs.items():
            match key:
                case 'id':
                    response = f"{response} and\n    Query Parameter: {key} set to '{value}'"
                case 'fragment_filename':
                    response = f"{response} and\n    Query Parameter: {key} set to '{value}'"
                case 'content_type':
                    response = f"{response} and\n    Query Parameter: {key} set to '{value}'"
                case 'options':
                    response = f"{response} and\n    Query Parameter: {key} set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def create_entity_from_file(kwargs) -> str:
        if 'filename' in kwargs:
            result = (f"Request creation of an entity from filename '{kwargs['filename']}'"
                      f" and Content-Type set to 'application/ld+json'")
            return result
        else:
            raise Exception(f"ERROR: expected filename attribute, but received {kwargs}")

    @staticmethod
    def call_api_endpoint_with_invalid_parameter(kwargs) -> str:
        expected_parameters = ['method', 'endpoint', 'filename', 'extraParam']

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Call API Endpoint with:"
        for key, value in kwargs.items():
            match key:
                case 'method':
                    response = f"{response} and\n    Method: {key} set to '{value}'"
                case 'endpoint':
                    response = f"{response} and\n    Endpoint: {key} set to '{value}'"
                case 'filename':
                    response = f"{response} and\n    Filename: {key} set to '{value}'"
                case 'extraParam':
                    response = f"{response} and\n    Extra param: {key} set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")
        return response

    @staticmethod
    def batch_update_entities(kwargs) -> str:
        if 'overwrite_option' not in kwargs:
            kwargs['overwrite_option'] = '${EMPTY}'

        if 'entities' in kwargs:
            result = (f"Request batch update operation over entity from filename '{kwargs['entities']}' "
                      f"with overwrite_option set to '{kwargs['overwrite_option']}' "
                      f"and Content-Type set to 'application/ld+json'")
            return result
        else:
            raise Exception(f"ERROR: expected overwrite_option or entities attributes, but received {kwargs}")

    @staticmethod
    def batch_upsert_entities(kwargs) -> str:
        if 'update_option' not in kwargs:
            kwargs['update_option'] = 'replace'

        if 'entities_to_be_upserted' in kwargs:
            result = (f"Request batch upsert operation over entity from filename '{kwargs['entities_to_be_upserted']}' "
                      f"with update_option set to '{kwargs['update_option']}' "
                      f"and Content-Type set to 'application/ld+json'")
            return result
        else:
            raise Exception(f"ERROR: expected update_option or entities_to_be_upserted attributes, but received {kwargs}")

    @staticmethod
    def batch_merge_entities(kwargs) -> str:
        if 'overwrite_option' not in kwargs:
            kwargs['overwrite_option'] = '${EMPTY}'

        if 'entities_to_be_merged' in kwargs:
            result = (f"Request batch merge operation over entity from entities_to_be_merged '{kwargs['entities_to_be_merged']}' "
                      f"with Content-Type set to 'application/ld+json'")
            return result
        else:
            raise Exception(f"ERROR, expected entities_to_be_merged attribute, but received {kwargs}")


    @staticmethod
    def list_contexts(kwargs) -> str:
        expected_parameters = ['details', 'kind']

        result = [x for x in kwargs if x not in expected_parameters]
        response = "List @contexts:"
        for key, value in kwargs.items():
            match key:
                case 'details':
                    response = f"{response} and\n    Query Parameter: details set to '{value}'"
                case 'kind':
                    response = f"{response} and\n    Query Parameter: kind set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def serve_a_context(kwargs) -> str:
        if 'details' not in kwargs:
            kwargs['details'] = '${EMPTY}'

        response = f"Serve a @context with contextID '{kwargs['contextId']}' and details '{kwargs['details']}'"

        return response

    @staticmethod
    def add_a_new_context(kwargs) -> str:
        return f"Add a new @context based on the payload defined in the file {kwargs['filename']}"

    @staticmethod
    def delete_a_context(kwargs) -> str:
        return f"Delete a @context whose 'URI' set to '{kwargs['uri']}'"

    @staticmethod
    def create_context_source_registration(kwargs) -> str:
        if 'context_source_registration_payload' in kwargs:
            result = (f"Create Context Source Registration Request with Content-Type set to 'application/ld+json' "
                      f"and payload set to '{kwargs['context_source_registration_payload']}'")
            return result
        else:
            raise Exception(f"ERROR: expected context_source_registration_payload attribute, but received {kwargs}")

    @staticmethod
    def wait_for_notification(kwargs) -> str:
        if 'timeout' in kwargs:
            result = f"Waiting for notification with timeout set to '{kwargs['timeout']}'"
            return result
        else:
            raise Exception(f"ERROR: expected timeout attribute, but received {kwargs}")

    @staticmethod
    def setup_initial_subscriptions(kwargs) -> str:
        result = """a subscription with id set to ${subscriptionId} 
           and status equals 'active'
           and timeInterval is set to '${timeInterval}'
           and watchedAttributes is 'Empty'
           and q is 'Empty'
           and geoQ is 'Empty'
           and with subscription.entity with type 'Building'
           and notification.endpoint.accept is 'application/json'
           
           When the timeinterval is reached at ${timeInterval} seconds
           the SUT needs to send out a notification to the client 
           sends a notification to the client every ${timeInterval} seconds"""

        return result

    @staticmethod
    def create_subscription_from_file(kwargs) -> str:
        if 'filename' in kwargs:
            result = (f"Create Subscription Request with Header['Content-Type'] set to 'application/ld+json' and\n "
                      f"payload defined in file: '{kwargs['filename']}'")
            return result
        else:
            raise Exception(f"ERROR: expected filename attribute, but received {kwargs}")

    @staticmethod
    def batch_request_entities_from_file(kwargs) -> str:
        if 'operation' in kwargs and 'filename' in kwargs:
            result = (f"Batch Entity Delete Request with operation set to '{kwargs['operation']}', Content-Type set to 'application/ld+json', and body set to '{kwargs['filename']}")
            return result
        else:
            raise Exception(f"ERROR: expected operation and filename attributes, but received {kwargs}")

    @staticmethod
    def request_entity_from_file(kwargs) -> str:
        if 'filename' in kwargs:
            result = f"Request Entity from file with filename set to '{kwargs['filename']}' and content-type set to 'application/ld+json'"
            return result
        else:
            raise Exception(f"ERROR: expected filename attribute, but received {kwargs}")

    @staticmethod
    def batch_create_entities(kwargs) -> str:
        if 'content_type' in kwargs:
            result = (f"Request Header['Content-Type'] set to '{kwargs['content_type']}' and\n "
                      f"payload set to a list of entities to be created")
            return result
        else:
            raise Exception(f"ERROR: expected content_type attribute, but received {kwargs}")

    @staticmethod
    def create_context_source_registration_with_return(kwargs) -> str:
        if 'filename' in kwargs:
            result = (f"Request Header['Content-Type'] set to 'application/ld+json' and\n "
                      f"payload defined in file: '{kwargs['filename']}'")
            return result
        else:
            raise Exception(f"ERROR: expected filename attribute, but received {kwargs}")

    @staticmethod
    def retrieve_entity(kwargs) -> str:
        expected_parameters = ['id', 'accept', 'attrs', 'context', 'geometry_property', 'options', 'format', 'lang',
                               'join', 'joinLevel', 'local']

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Retrieve Entity Request:"
        for key, value in kwargs.items():
            match key:
                case 'id':
                    response = f"{response} and\n    Query Parameter: id set to '{value}'"
                case 'accept':
                    response = f"{response} and\n    Query Parameter: accept set to '{value}'"
                case 'attrs':
                    response = f"{response} and\n    Query Parameter: attrs set to '{value}'"
                case 'context':
                    response = (f"{response} and\n    Query Parameter: Link set to "
                                f"'<${value}>; rel=\"http://www.w3.org/ns/json-ld#context\";type=\"application/ld+json\"'")
                case 'geometryProperty':
                    response = f"{response} and\n    Query Parameter: geometryProperty set to '{value}'"
                case 'options':
                    response = f"{response} and\n    Query Parameter: options set to '{value}'"
                case 'format':
                    response = f"{response} and\n    Query Parameter: format set to '{value}'"
                case 'lang':
                    response = f"{response} and\n    Query Parameter: lang set to '{value}'"
                case 'join':
                    response = f"{response} and\n    Query Parameter: join set to '{value}'"
                case 'joinLevel':
                    response = f"{response} and\n    Query Parameter: joinLevel set to '{value}'"
                case 'pick':
                    response = f"{response} and\n    Query Parameter: pick set to '{value}'"
                case 'omit':
                    response = f"{response} and\n    Query Parameter: omit set to '{value}'"
                case 'local':
                    response = f"{response} and\n    Query Parameter: local set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def modify_attribute_instance_from_temporal_entity(kwargs) -> str:
        expected_parameters = ['temporal_entity_id', 'attributeId', 'instanceId', 'fragment_filename', 'content_type', 'context']

        if 'context' not in kwargs:
            kwargs['context'] = '${EMPTY}'

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Modify Attribute Instance from Temporal Entity:"
        for key, value in kwargs.items():
            match key:
                case 'temporal_entity_id':
                    response = f"{response} and\n    Query Parameter: temporal_entity_id set to '{value}'"
                case 'attributeId':
                    response = f"{response} and\n    Query Parameter: attributeId set to '{value}'"
                case 'instanceId':
                    response = f"{response} and\n    Query Parameter: instanceId set to '{value}'"
                case 'fragment_filename':
                    response = f"{response} and\n    Query Parameter: fragment_filename set to '{value}'"
                case 'content_type':
                    response = f"{response} and\n    Query Parameter: content_type set to '{value}'"
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def delete_attribute_instance_from_temporal_entity(kwargs) -> str:
        expected_parameters = ['temporal_entity_id', 'attributeId', 'instanceId', 'content_type', 'context']

        if 'context' not in kwargs:
            kwargs['context'] = '${EMPTY}'

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Delete Attribute Instance from Temporal Entity:"
        for key, value in kwargs.items():
            match key:
                case 'temporal_entity_id':
                    response = f"{response} and\n    Query Parameter: temporal_entity_id set to '{value}'"
                case 'attributeId':
                    response = f"{response} and\n    Query Parameter: attributeId set to '{value}'"
                case 'instanceId':
                    response = f"{response} and\n    Query Parameter: instanceId set to '{value}'"
                case 'content_type':
                    response = f"{response} and\n    Query Parameter: content_type set to '{value}'"
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def delete_attribute_from_temporal_entity(kwargs) -> str:
        expected_parameters = ['entityId', 'attributeId', 'content_type', 'datasetId', 'deleteAll', 'context']

        if 'context' not in kwargs:
            kwargs['context'] = '${EMPTY}'

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Delete Attribute From Temporal Entity:"
        for key, value in kwargs.items():
            match key:
                case 'entityId':
                    response = f"{response} and\n    Query Parameter: entityId set to '{value}'"
                case 'attributeId':
                    response = f"{response} and\n    Query Parameter: attributeId set to '{value}'"
                case 'content_type':
                    response = f"{response} and\n    Query Parameter: content_type set to '{value}'"
                case 'datasetId':
                    response = f"{response} and\n    Query Parameter: datasetId set to '{value}'"
                case 'deleteAll':
                    response = f"{response} and\n    Query Parameter: deleteAll set to '{value}'"
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def create_or_update_temporal_representation_of_entity_selecting_content_type(kwargs) -> str:
        expected_parameters = ['temporal_entity_representation_id', 'filename', 'content_type', 'accept']

        if 'accept' not in kwargs:
            kwargs['accept'] = '${EMPTY}'

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Create or Update Temporal Representation of Entity Selecting Content Type:"
        for key, value in kwargs.items():
            match key:
                case 'temporal_entity_representation_id':
                    response = f"{response} and\n    Query Parameter: temporal_entity_representation_id set to '{value}'"
                case 'filename':
                    response = f"{response} and\n    Query Parameter: filename set to '{value}'"
                case 'content_type':
                    response = f"{response} and\n    Query Parameter: content_type set to '{value}'"
                case 'accept':
                    response = f"{response} and\n    Query Parameter: accept set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute(w) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def create_temporal_representation_of_entity_selecting_content_type(kwargs) -> str:
        expected_parameters = ['filename', 'content_type']

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Create Temporal Representation of Entity Selecting Content Type:"
        for key, value in kwargs.items():
            match key:
                case 'filename':
                    response = f"{response} and\n    Query Parameter: filename set to '{value}'"
                case 'content_type':
                    response = f"{response} and\n    Query Parameter: content_type set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def delete_entity_attributes(kwargs) -> str:
        expected_parameters = ['entityId', 'attributeId', 'datasetId', 'deleteAll', 'context']

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Delete Entity Attributes:"
        for key, value in kwargs.items():
            match key:
                case 'entityId':
                    response = f"{response} and\n    Query Parameter: entityId set to '{value}'"
                case 'attributeId':
                    response = f"{response} and\n    Query Parameter: attributeId set to '{value}'"
                case 'datasetId':
                    response = f"{response} and\n    Query Parameter: datasetId set to '{value}'"
                case 'deleteAll':
                    response = f"{response} and\n    Query Parameter: deleteAll set to '{value}'"
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def retrieve_attributes(kwargs) -> str:
        expected_parameters = ['details', 'accept', 'context']

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Retrieve attributes:"
        for key, value in kwargs.items():
            match key:
                case 'details':
                    response = f"{response} and\n    Query Parameter: details set to '{value}'"
                case 'accept':
                    response = f"{response} and\n    Query Parameter: accept set to '{value}'"
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def batch_delete_entities(kwargs) -> str:
        expected_parameters = ['entities_ids_to_be_deleted', 'teardown']

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Batch Delete Entities:"
        for key, value in kwargs.items():
            match key:
                case 'entities_ids_to_be_deleted':
                    response = f"{response} and\n    Query Parameter: entities_ids_to_be_deleted set to '{value}'"
                case 'teardown':
                    response = f"{response} and\n    Query Parameter: teardown set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def retrieve_entity_types(kwargs) -> str:
        expected_parameters = ['details', 'accept', 'context']

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Retrieve entity types:"
        for key, value in kwargs.items():
            match key:
                case 'details':
                    response = f"{response} and\n    Query Parameter: details set to '{value}'"
                case 'accept':
                    response = f"{response} and\n    Query Parameter: accept set to '{value}'"
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def retrieve_subscription(kwargs) -> str:
        # if 'accept' in kwargs:
        #     return f"Request a subscription\nHeader['Accept'] set to '{kwargs['accept']}'"
        # else:
        #     return "Request a subscription"
        expected_parameters = ['id', 'accept', 'context', 'content_type']

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Subscription Retrieve with the following data:"
        for key, value in kwargs.items():
            match key:
                case 'id':
                    response = f"{response} and\n    Query Parameter: id set to '{value}'"
                case 'accept':
                    response = f"{response} and\n    Query Parameter: accept set to '{value}'"
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case 'content_type':
                    response = f"{response} and\n    Query Parameter: content_type set to '{value}'"
                # If an exact match is not confirmed, this last case will be used if provided
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    def query_context_source_registrations_with_return(kwargs) -> str:
        if 'type' in kwargs and 'accept' in kwargs:
            result = "Request a Context Source Registration with Return"

            if kwargs['type'] != '':
                result = f"{result}\nEntity Type set to '{kwargs['type']}'"

            if kwargs['accept'] != '':
                result = f"{result}\nHeader['Accept'] set to '{kwargs['accept']}'"
        else:
            result = "Request a Context Source Registration with Return"

        return result

    def query_temporal_representation_of_entities_with_return(kwargs) -> str:
        return "Request a Temporal Representation of Entities with Return"

    def partial_update_entity_attributes(kwargs) -> str:
        expected_parameters = ['entityId', 'attributeId', 'fragment_filename', 'content_type', 'accept', 'context']

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Request Partial Update Entity Attributes"
        for key, value in kwargs.items():
            match key:
                case 'entityId':
                    response = f"{response} and\n    Query Parameter: entityId set to '{value}'"
                case 'attributeId':
                    response = f"{response} and\n    Query Parameter: AttributeId set to '{value}'"
                case 'fragment_filename':
                    response = f"{response} and\n    Query Parameter: fragment_filename set to '{value}'"
                case 'content_type':
                    response = f"{response} and\n    Query Parameter: content_type set to '{value}'"
                case 'accept':
                    response = f"{response} and\n    Query Parameter: accept set to '{value}'"
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                # If an exact match is not confirmed, this last case will be used if provided
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    def update_subscription(kwargs) -> str:
        if 'context' in kwargs and 'content' in kwargs and 'filename' in kwargs:
            context = kwargs['context']
            if context == '':
                return (f"Request Update Subscription and \n"
                        f"Header['Content-Type'] set to '{kwargs['content']}' and\n"
                        f"Payload defined in file '{kwargs['filename']}'")
            else:
                return (f"Request Update Subscription and \n"
                        f"Header['Link'] contain the context '{kwargs['context']}' and\n"
                        f"Header['Content-Type'] set to '{kwargs['content']}' and\n"
                        f"Payload defined in file '{kwargs['filename']}'")
        else:
            raise Exception(f"ERROR: expected context, content and filename attributes, but received {kwargs}")

    @staticmethod
    def query_context_source_registration_subscriptions(kwargs) -> str:
        expected_parameters = ['context', 'limit', 'page', 'accept']

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Query Context Source Registration Subscriptions"
        for key, value in kwargs.items():
            match key:
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case 'limit':
                    response = f"{response} and\n    Query Parameter: limit set to '{value}'"
                case 'page':
                    response = f"{response} and\n    Query Parameter: page set to '{value}'"
                case 'accept':
                    response = f"{response} and\n    Query Parameter: accept set to '{value}'"
                # If an exact match is not confirmed, this last case will be used if provided
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def query_temporal_representation_of_entities(kwargs) -> str:
        # This function is a little bit special because we have a number of parameters not always defined and not always
        # in the same position, so we make a different analysis to extract the values
        expected_parameters = ['context', 'entity_types', 'entity_ids', 'entity_id_pattern',
                               'ngsild_query', 'csf', 'georel', 'geometry',
                               'coordinates', 'geoproperty', 'timerel', 'timeAt','endTimeAt',
                               'attrs', 'limit', 'lastN', 'accept', 'options', 'format', 'datasetId', 'aggrMethods',
                               'aggrPeriodDuration', 'pick', 'omit', 'orderBy', 'timeproperty', 'local']

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Query Temporal Representation of Entities"
        for key, value in kwargs.items():
            match key:
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case 'entity_types':
                    response = f"{response} and\n    Query Parameter: entity_types set to '{value}'"
                case 'entity_ids':
                    response = f"{response} and\n    Query Parameter: entity_ids set to '{value}'"
                case 'entity_id_pattern':
                    response = f"{response} and\n    Query Parameter: entity_id_pattern set to '{value}'"
                case 'ngsild_query':
                    response = f"{response} and\n    Query Parameter: ngsild_query set to '{value}'"
                case 'csf':
                    response = f"{response} and\n    Query Parameter: csf set to '{value}'"
                case 'georel':
                    response = f"{response} and\n    Query Parameter: georel set to '{value}'"
                case 'geometry':
                    response = f"{response} and\n    Query Parameter: geometry set to '{value}'"
                case 'coordinates':
                    response = f"{response} and\n    Query Parameter: coordinates set to '{value}'"
                case 'geoproperty':
                    response = f"{response} and\n    Query Parameter: geoproperty set to '{value}'"
                case 'timerel':
                    response = f"{response} and\n    Query Parameter: timerel set to '{value}'"
                case 'timeAt':
                    response = f"{response} and\n    Query Parameter: timeAt set to '{value}'"
                case 'endTimeAt':
                    response = f"{response} and\n    Query Parameter: endTimeAt set to '{value}'"
                case 'timeproperty':
                    response = f"{response} and\n    Query Parameter: timeproperty set to '{value}'"
                case 'attrs':
                    response = f"{response} and\n    Query Parameter: attrs set to '{value}'"
                case 'limit':
                    response = f"{response} and\n    Query Parameter: limit set to '{value}'"
                case 'lastN':
                    if re.search(pattern=r'\d+', string=value):
                        value = re.search(pattern=r'\d+', string=value).group()
                    response = f"{response} and\n    Query Parameter: lastN set to '{value}'"
                case 'accept':
                    response = f"{response} and\n    Query Parameter: accept set to '{value}'"
                case 'options':
                    response = f"{response} and\n    Query Parameter: options set to '{value}'"
                case 'format':
                    response = f"{response} and\n    Query Parameter: format set to '{value}'"
                case 'aggrMethods':
                    response = f"{response} and\n    Query Parameter: aggrMethods set to '{value}'"
                case 'aggrPeriodDuration':
                    response = f"{response} and\n    Query Parameter: aggrPeriodDuration set to '{value}'"
                case 'datasetId':
                    response = f"{response} and\n    Query Parameter: datasetId set to '{value}'"
                case 'pick':
                    response = f"{response} and\n    Query Parameter: pick set to '{value}'"
                case 'omit':
                    response = f"{response} and\n    Query Parameter: omit set to '{value}'"
                case 'orderBy':
                    response = f"{response} and\n    Query Parameter: orderBy set to '{value}'"
                case 'local':
                    response = f"{response} and\n    Query Parameter: local set to '{value}'"

            # If an exact match is not confirmed, this last case will be used if provided
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def retrieve_temporal_representation_of_entity(kwargs) -> str:
        expected_parameters = ['temporal_entity_representation_id', 'attrs', 'options',
                               'context', 'timerel', 'timeAt', 'endTimeAt', 'lastN', 'accept',
                               'aggrMethods', 'aggrPeriodDuration', 'timeproperty', 'pick', 'omit']
        result = [x for x in kwargs if x not in expected_parameters]
        response = "Retrieve Temporal Representation of Entity"
        for key, value in kwargs.items():
            match key:
                case 'temporal_entity_representation_id':
                    response = f"{response} and\n    Query Parameter: id set to '{value}'"
                case 'attrs':
                    response = f"{response} and\n    Query Parameter: attrs set to '{value}'"
                case 'options':
                    response = f"{response} and\n    Query Parameter: options set to '{value}'"
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case 'timerel':
                    response = f"{response} and\n    Query Parameter: timerel set to '{value}'"
                case 'timeAt':
                    response = f"{response} and\n    Query Parameter: timeAt set to '{value}'"
                case 'endTimeAt':
                    response = f"{response} and\n    Query Parameter: endTimeAt set to '{value}'"
                case 'lastN':
                    if re.search(pattern=r'\d+', string=value):
                        value = re.search(pattern=r'\d+', string=value).group()
                    response = f"{response} and\n    Query Parameter: lastN set to '{value}'"
                case 'accept':
                    response = f"{response} and\n    Query Parameter: accept set to '{value}'"
                case 'aggrMethods':
                    response = f"{response} and\n    Query Parameter: aggrMethods set to '{value}'"
                case 'aggrPeriodDuration':
                    response = f"{response} and\n    Query Parameter: aggrPeriodDuration set to '{value}'"
                case 'timeproperty':
                    response = f"{response} and\n    Query Parameter: timeproperty set to '{value}'"
                case 'pick':
                    response = f"{response} and\n    Query Parameter: pick set to '{value}'"
                case 'omit':
                    response = f"{response} and\n    Query Parameter: omit set to '{value}'"
                # If an exact match is not confirmed, this last case will be used if provided
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def retrieve_attribute(kwargs) -> str:
        if 'attribute_name' in kwargs:
            return f"Retrieve Attribute with attributeName set to '{kwargs['attribute_name']}'"

    @staticmethod
    def retrieve_entity_type(kwargs) -> str:
        result = "Retrieve Entity Type"
        if 'type' in kwargs:
            result = f"{result}, with type set to '{kwargs['type']}'"

        if 'context' in kwargs and kwargs['context'] != '':
            result = f"{result}, with Header['Link'] containing '{kwargs['context']}'"

        if 'type' not in kwargs or 'context' not in kwargs:
            raise Exception(f"ERROR: expected type and context attributes, received '{kwargs}'")

        return result

    @staticmethod
    def query_entities(kwargs) -> str:
        expected_parameters = ['entity_ids', 'entity_types', 'accept',
                               'attrs', 'context', 'geoproperty',
                               'options', 'limit', 'entity_id_pattern',
                               'scopeq', 'georel', 'coordinates', 'geometry', 'count' , 'q' , 'datasetId',
                               'join', 'joinLevel', 'pick', 'omit', 'local', 'orderBy', 'local']

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Get Entities Request:"
        for key, value in kwargs.items():
            match key:
                case 'entity_ids':
                    response = f"{response} and\n    Query Parameter: entity_ids set to '{value}'"
                case 'entity_types':
                    response = f"{response} and\n    Query Parameter: entity_types set to '{value}'"
                case 'accept':
                    response = f"{response} and\n    Query Parameter: accept set to '{value}'"
                case 'attrs':
                    response = f"{response} and\n    Query Parameter: attrs set to '{value}'"
                case 'context':
                    response = (f"{response} and\n    Query Parameter: Link set to "
                                f"'<${value}>; rel=\"http://www.w3.org/ns/json-ld#context\";type=\"application/ld+json\"'")
                case 'geoproperty':
                    response = f"{response} and\n    Query Parameter: geoproperty set to '{value}'"
                case 'options':
                    response = f"{response} and\n    Query Parameter: options set to '{value}'"
                case 'limit':
                    response = f"{response} and\n    Query Parameter: limit set to '{value}'"
                case 'entity_id_pattern':
                    response = f"{response} and\n    Query Parameter: entity_id_pattern set to '{value}'"
                case 'scopeq':
                    response = f"{response} and\n    Query Parameter: scopeq set to '{value}'"
                case 'georel':
                    response = f"{response} and\n    Query Parameter: georel set to '{value}'"
                case 'coordinates':
                    response = f"{response} and\n    Query Parameter: coordinates set to '{value}'"
                case 'geometry':
                    response = f"{response} and\n    Query Parameter: geometry set to '{value}'"
                case 'count':
                    response = f"{response} and\n    Query Parameter: count set to '{value}'"
                case 'q':
                    response = f"{response} and\n    Query Parameter: q set to '{value}'"
                case 'datasetId':
                    response = f"{response} and\n    Query Parameter: datasetId set to '{value}'"
                case 'join':
                    response = f"{response} and\n    Query Parameter: join set to '{value}'"
                case 'joinLevel':
                    response = f"{response} and\n    Query Parameter: joinLevel set to '{value}'"
                case 'pick':
                    response = f"{response} and\n    Query Parameter: pick set to '{value}'"
                case 'omit':
                    response = f"{response} and\n    Query Parameter: omit set to '{value}'"
                case 'local':
                    response = f"{response} and\n    Query Parameter: local set to '{value}'"
                case 'orderBy':
                    response = f"{response} and\n    Query Parameter: orderBy set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    def query_entities_via_post(kwargs) -> str:
        expected_parameters = ['entities', 'content_type', 'accept', 'attrs', 'geometry_property', 'join',
                               'joinLevel', 'options', 'local']

        if 'content_type' not in kwargs:
            kwargs['content_type'] = 'application/json'

        if 'accept' not in kwargs:
            kwargs['accept'] = 'application/json'

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Get Entities Via POST Request:"
        for key, value in kwargs.items():
            match key:
                case 'entities':
                    response = f"{response} and\n    Query Parameter: entities set to '{value}'"
                case 'content_type':
                    response = f"{response} and\n    Query Parameter: content_type set to '{value}'"
                case 'accept':
                    response = f"{response} and\n    Query Parameter: accept set to '{value}'"
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case 'attrs':
                    response = f"{response} and\n    Query Parameter: attrs set to '{value}'"
                case 'geometry_property':
                    response = f"{response} and\n    Query Parameter: geoproperty set to '{value}'"
                case 'join':
                    response = f"{response} and\n    Query Parameter: join set to '{value}'"
                case 'joinLevel':
                    response = f"{response} and\n    Query Parameter: joinLevel set to '{value}'"
                case 'options':
                    response = f"{response} and\n    Query Parameter: options set to '{value}'"
                case 'local':
                    response = f"{response} and\n    Query Parameter: local set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    def query_temporal_representation_of_entities_via_post(kwargs) -> str:
        expected_parameters = ['query_file_name', 'content_type', 'context']

        if 'content_type' not in kwargs:
            kwargs['content_type'] = 'application/json'

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Query Temporal Representation of Entities Via POST Request:"
        for key, value in kwargs.items():
            match key:
                case 'query_file_name':
                    response = f"{response} and\n    Query Parameter: query_file_name set to '{value}'"
                case 'content_type':
                    response = f"{response} and\n    Query Parameter: content_type set to '{value}'"
                case 'context':
                    response = (f"{response} and\n    Query Parameter: Link set to "
                                f"'<${value}>; rel=\"http://www.w3.org/ns/json-ld#context\";type=\"application/ld+json\"'")
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def delete_entity_by_id(kwargs) -> str:
        if 'id' in kwargs:
            return f"Delete Entity Request with id set to '{kwargs['id']}'"

    @staticmethod
    def replace_entity(kwargs) -> str:
        expected_parameters = ['entity_id', 'filename', 'content_type', 'context']

        if 'context' not in kwargs:
            kwargs['context'] = '${EMPTY}'

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Replace Entity"
        for key, value in kwargs.items():
            match key:
                case 'entity_id':
                    response = f"{response} and\n    Query Parameter: entity_id set to '{value}'"
                case 'filename':
                    response = f"{response} and\n    Query Parameter: filename set to '{value}'"
                case 'content_type':
                    response = f"{response} and\n    Query Parameter: content_type set to '{value}'"
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def replace_entity_selecting_content_type(kwargs) -> str:
        expected_parameters = ['entity_id', 'entity_fragment', 'content_type', 'context']

        if 'context' not in kwargs:
            kwargs['context'] = '${EMPTY}'

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Replace Entity Selecting Content Type"
        for key, value in kwargs.items():
            match key:
                case 'entity_id':
                    response = f"{response} and\n    Query Parameter: entity_id set to '{value}'"
                case 'entity_fragment':
                    response = f"{response} and\n    Query Parameter: entity_fragment set to '{value}'"
                case 'content_type':
                    response = f"{response} and\n    Query Parameter: content_type set to '{value}'"
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def purge_entities(kwargs) -> str:
        expected_parameters = ['type', 'id', 'q', 'keep', 'drop', 'context', 'local']

        if 'context' not in kwargs:
            kwargs['context'] = '${EMPTY}'

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Purge Entities"
        for key, value in kwargs.items():
            match key:
                case 'type':
                    response = f"{response} and\n    Query Parameter: type set to '{value}'"
                case 'id':
                    response = f"{response} and\n    Query Parameter: id set to '{value}'"
                case 'q':
                    response = f"{response} and\n    Query Parameter: q set to '{value}'"
                case 'keep':
                    response = f"{response} and\n    Query Parameter: keep set to '{value}'"
                case 'drop':
                    response = f"{response} and\n    Query Parameter: drop set to '{value}'"
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def merge_entity(kwargs) -> str:
        expected_parameters = ['entity_id', 'entity_filename', 'content_type', 'context']

        if 'context' not in kwargs:
            kwargs['context'] = '${EMPTY}'

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Merge Entity"
        for key, value in kwargs.items():
            match key:
                case 'entity_id':
                    response = f"{response} and\n    Query Parameter: entity_id set to '{value}'"
                case 'entity_filename':
                    response = f"{response} and\n    Query Parameter: entity_filename set to '{value}'"
                case 'content_type':
                    response = f"{response} and\n    Query Parameter: content_type set to '{value}'"
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def replace_attribute_selecting_content_type(kwargs) -> str:
        expected_parameters = ['entity_id', 'attr_id', 'attribute_fragment', 'content_type', 'context']

        if 'context' not in kwargs:
            kwargs['context'] = '${EMPTY}'

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Replace Attribute Selecting Content Type"
        for key, value in kwargs.items():
            match key:
                case 'entity_id':
                    response = f"{response} and\n    Query Parameter: entity_id set to '{value}'"
                case 'attr_id':
                    response = f"{response} and\n    Query Parameter: attr_id set to '{value}'"
                case 'attribute_fragment':
                    response = f"{response} and\n    Query Parameter: attribute_fragment set to '{value}'"
                case 'content_type':
                    response = f"{response} and\n    Query Parameter: content_type set to '{value}'"
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case _:
                    raise Exception(f"ERROR: unexpected attribute {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def delete_subscription(kwargs) -> str:
        if 'id' in kwargs:
            return f"Delete Subscription with id set to '{kwargs['id']}'"

    @staticmethod
    def query_subscriptions(kwargs) -> str:
        expected_parameters = ['context', 'limit', 'offset', 'accept']

        result = [x for x in kwargs if x not in expected_parameters]
        response = 'Query Subscription Request with data:'
        for key, value in kwargs.items():
            match key:
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case 'limit':
                    response = f"{response} and\n    Query Parameter: limit set to '{value}'"
                case 'offset':
                    response = f"{response} and\n    Query Parameter: offset set to '{value}'"
                case 'accept':
                    response = f"{response} and\n    Query Parameter: accept set to '{value}'"
                # If an exact match is not confirmed, this last case will be used if provided
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def retrieve_context_source_registration_subscription(kwargs) -> str:
        expected_parameters = ['subscription_id', 'context', 'accept']

        result = [x for x in kwargs if x not in expected_parameters]
        response = 'Retrieve Context Source Registration Subscription with data:'
        for key, value in kwargs.items():
            match key:
                case 'subscription_id':
                    response = f"{response} and\n    Query Parameter: subscription id set to '{value}'"
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case 'accept':
                    response = f"{response} and\n    Query Parameter: accept set to '{value}'"
                # If an exact match is not confirmed, this last case will be used if provided
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def delete_context_source_registration_with_return(kwargs) -> str:
        if 'id' in kwargs:
            return f"Delete Context Source Registration with id set to '{kwargs['id']}'"

    @staticmethod
    def update_context_source_registration(kwargs) -> str:
        if 'context_source_registration_id' in kwargs and 'update_fragment' in kwargs:
            return (f"Update Context Source Registration "
                    f"with CSR Id set to '{kwargs['context_source_registration_id']}' and "
                    f"subscription update fragment set to '{kwargs['update_fragment']}'")
        else:
            raise Exception(f"ERROR: expected 'context_source_registration_id' and 'update_fragment'"
                            f" but received: {kwargs}")

    @staticmethod
    def update_context_source_registration_from_file(kwargs) -> str:
        if 'context_source_registration_id' in kwargs and 'filename' in kwargs:
            return (f"Update Context Source Registration "
                    f"with CSR Id set to '{kwargs['context_source_registration_id']}' and "
                    f"subscription update from file '{kwargs['filename']}'")
        else:
            raise Exception(f"ERROR: expected 'context_source_registration_id' and 'filename'"
                            f" but received: {kwargs}")

    @staticmethod
    def update_context_source_registration_subscription(kwargs) -> str:
        if 'subscription_id' in kwargs and 'subscription_update_fragment' in kwargs:
            return (f"Update Context Source Registration Subscription "
                    f"with subscription id set to '{kwargs['subscription_id']}' and "
                    f"subscription update fragment set to '{kwargs['subscription_update_fragment']}'")
        else:
            raise Exception(f"ERROR: expected 'subscription_id' and 'subscription_update_fragment' but received: {kwargs}")

    @staticmethod
    def update_context_source_registration_subscription_from_file(kwargs) -> str:
        if 'subscription_id' in kwargs and 'subscription_update_fragment' in kwargs:
            return (f"Update Context Source Registration Subscription from file "
                    f"with subscription id set to '{kwargs['subscription_id']}' and "
                    f"subscription update fragment set to '{kwargs['subscription_update_fragment']}'")
        else:
            raise Exception(f"ERROR: expected 'subscription_id' and 'subscription_update_fragment' but received: {kwargs}")

    @staticmethod
    def retrieve_context_source_registration(kwargs) -> str:
        expected_parameters = ['context_source_registration_id', 'context', 'accept']

        result = [x for x in kwargs if x not in expected_parameters]
        response = 'Retrieve Context Source Registration with data:'
        for key, value in kwargs.items():
            match key:
                case 'context_source_registration_id':
                    response = f"{response} and\n    Query Parameter: context source registration id set to '{value}'"
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case 'accept':
                    response = f"{response} and\n    Query Parameter: accept set to '{value}'"
                # If an exact match is not confirmed, this last case will be used if provided
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def query_context_source_registrations(kwargs) -> str:
        expected_parameters = ['context', 'id', 'type', 'idPattern', 'attrs',
                               'q', 'csf', 'georel', 'geometry',
                               'coordinates', 'geoproperty', 'timeproperty', 'timerel',
                               'timeAt', 'limit', 'offset', 'accept']

        # kwargs = {key: kwargs.get(key, '${EMPTY}') for key in expected_parameters}

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Retrieve Temporal Representation of Entity"
        for key, value in kwargs.items():
            match key:
                case 'context':
                    response = f"{response} and\n    Query Parameter: context set to '{value}'"
                case 'id':
                    response = f"{response} and\n    Query Parameter: id set to '{value}'"
                case 'type':
                    response = f"{response} and\n    Query Parameter: type set to '{value}'"
                case 'attrs':
                    response = f"{response} and\n    Query Parameter: attrs set to '{value}'"
                case 'idPattern':
                    response = f"{response} and\n    Query Parameter: idPattern set to '{value}'"
                case 'q':
                    response = f"{response} and\n    Query Parameter: q set to '{value}'"
                case 'csf':
                    response = f"{response} and\n    Query Parameter: csf set to '{value}'"
                case 'georel':
                    response = f"{response} and\n    Query Parameter: georel set to '{value}'"
                case 'geometry':
                    response = f"{response} and\n    Query Parameter: geometry set to '{value}'"
                case 'coordinates':
                    response = f"{response} and\n    Query Parameter: coordinates set to '{value}'"
                case 'geoproperty':
                    response = f"{response} and\n    Query Parameter: geoproperty set to '{value}'"
                case 'timeproperty':
                    response = f"{response} and\n    Query Parameter: timeproperty set to '{value}'"
                case 'timerel':
                    response = f"{response} and\n    Query Parameter: timerel set to '{value}'"
                case 'timeAt':
                    response = f"{response} and\n    Query Parameter: timeAt set to '{value}'"
                case 'limit':
                    response = f"{response} and\n    Query Parameter: limit set to '{value}'"
                case 'offset':
                    response = f"{response} and\n    Query Parameter: offset set to '{value}'"
                case 'accept':
                    response = f"{response} and\n    Query Parameter: accept set to '{value}'"
                # If an exact match is not confirmed, this last case will be used if provided
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def update_context_source_registration_with_return(kwargs) -> str:
        if 'id' in kwargs and 'filename' in kwargs and 'content' in kwargs:
            return (f"Update Context Source Registration with id set to '{kwargs['id']}' "
                    f"and registration update fragment set to '{kwargs['filename']}' "
                    f"and content-type set to '{kwargs['content']}'")

    @staticmethod
    def delete_temporal_representation_of_entity_with_returning_response(kwargs) -> str:
        if 'id' in kwargs:
            return f"Delete Temporal Representation Of Entity With Returning Response with id set to '{kwargs['id']}'"

    @staticmethod
    def retrieve_context_source_registration_subscription_2(kwargs) -> str:
        if 'id' in kwargs:
            return f"Retrieve Context Source Registration Subscription with id set to '{kwargs['id']}'"
        else:
            raise Exception(f"ERROR: expected 'id' but received: '{kwargs}'")

    @staticmethod
    def delete_context_source_registration_subscription(kwargs) -> str:
        if 'id' in kwargs:
            return f"Delete Context Source Registration Subscription with id set to '{kwargs['id']}'"
        else:
            raise Exception(f"ERROR: expected 'id' but received: '{kwargs}'")

    @staticmethod
    def create_context_source_registration_subscription(kwargs) -> str:
        if 'filename' in kwargs:
            return (f"Create Context Source Registration Subscription with filename set to '{kwargs['filename']}', "
                    f"accept set to '${{EMPTY}}', and content-type set to 'application/ld+json'")
        else:
            raise Exception(f"ERROR: expected 'filename' but received: '{kwargs}'")

    @staticmethod
    def append_attribute_to_temporal_entity(kwargs) -> str:
        expected_parameters = ['id', 'fragment_filename', 'content_type']

        result = [x for x in kwargs if x not in expected_parameters]
        response = 'Append Attribute to Temporal Entity'
        for key, value in kwargs.items():
            match key:
                case 'id':
                    response = f"{response} and\n    Query Parameter: id set to '{value}'"
                case 'fragment_filename':
                    response = f"{response} and\n    Query Parameter: fragment_filename set to '{value}'"
                case 'content_type':
                    response = f"{response} and\n    Query Parameter: content_type set to '{value}'"
                # If an exact match is not confirmed, this last case will be used if provided
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def append_entity_attributes(kwargs) -> str:
        expected_parameters = ['id', 'fragment_filename', 'content_type']

        result = [x for x in kwargs if x not in expected_parameters]
        response = 'Append Entity Attributes'
        for key, value in kwargs.items():
            match key:
                case 'id':
                    response = f"{response} and\n    Query Parameter: id set to '{value}'"
                case 'fragment_filename':
                    response = f"{response} and\n    Query Parameter: fragment_filename set to '{value}'"
                case 'content_type':
                    response = f"{response} and\n    Query Parameter: content_type set to '{value}'"
                # If an exact match is not confirmed, this last case will be used if provided
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}, the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    @staticmethod
    def update_entity_attributes(kwargs) -> str:
        expected_parameters = ['id', 'fragment_filename', 'content_type']

        result = [x for x in kwargs if x not in expected_parameters]
        response = 'Update Entity Attributes'
        for key, value in kwargs.items():
            match key:
                case 'id':
                    response = f"{response} and\n    Query Parameter: id set to '{value}'"
                case 'fragment_filename':
                    response = f"{response} and\n    Query Parameter: fragment_filename set to '{value}'"
                case 'content_type':
                    response = f"{response} and\n    Query Parameter: content_type set to '{value}'"
                # If an exact match is not confirmed, this last case will be used if provided
                case _:
                    raise Exception(f"ERROR: unexpected attribute(s) {result}', the attributes expected are "
                                    f"{expected_parameters}, but received: {kwargs}")

        return response

    def get_value(self, params, param_position, param_key):
        data = [x for x in params if f'{param_key}=' in x]

        if len(data) == 1:
            # The name of the attribute is passed to the function in the form attribute=value
            data = data[0]
            data = data.split('=')
            if data[0] != param_key:
                return ''

            data = data[1]
        elif len(data) == 0:
            # There is no attribute=something therefore we have to apply the position
            try:
                data = params[param_position]

                # Workaround
                if 'accept' in data and param_key != 'accept':
                    data = ''
            except IndexError:
                return ''

        return self.get_value_simple(data=data)

    def get_value_simple(self, data):
        try:
            value = self.variables[data]
            return value
        except KeyError:
            try:
                value = self.apiutils_variables[data]
                return value
            except KeyError:
                try:
                    value = self.config_file.get_variable(variable=data)
                    return value
                except KeyError:
                    try:
                        aux = self.template_params_value[self.name]
                        value = aux[data]
                        return value
                    except KeyError:
                        return data
