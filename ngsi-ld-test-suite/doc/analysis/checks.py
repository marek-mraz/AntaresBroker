from http import HTTPStatus


class Checks:
    def __init__(self):
        self.checks = {
            'Check Response Status Code':
                Checks.check_response_status_code,
            'Check Response Body Is Empty':
                Checks.check_response_body_is_empty,
            'Check Response Body Containing Array Of URIs set to':
                Checks.check_response_body_containing_array_of_uris_set_to,
            'Check Response Body Containing Entities URIS set to' :
                 Checks.check_response_body_containing_entities_uris_set_to,
            'Check Created Resources Set To':
                Checks.check_created_resources_set_to,
            'Check Response Headers Containing Content-Type set to':
                Checks.check_response_headers_containing_content_type_set_to,
            'Check Response Headers Link Not Empty':
                Checks.check_response_headers_link_not_empty,
            'Check Response Headers Containing URI set to':
                Checks.check_response_headers_containing_uri_set_to,
            'Check Response Headers ID Not Empty':
                Checks.check_response_headers_id_not_empty,
            'Check Response Body Containing an Attribute set to':
                Checks.check_response_body_containing_an_attribute_set_to,
            'Check Response Body Containing Entity element':
                Checks.check_response_body_containing_entity_element,
            'Check Response Body Containing List Containing Entity Elements':
                Checks.check_response_body_containing_list_containing_entity_elements,
            'Check Response Body Containing List Containing Entity Elements With Different Types':
                Checks.check_response_body_containing_list_containing_entity_elements_with_different_types,
            'Check Response Body Containing EntityTemporal element':
                Checks.check_response_body_containing_entitytemporal_element,
            'Check Response Body Containing List Containing EntityTemporal elements':
                Checks.check_response_body_containing_list_containing_entitytemporal_elements,
            'Check Response Body Containing Subscription element':
                Checks.check_response_body_containing_subscription_element,
            'Check Response Body Containing List Containing Subscription elements':
                Checks.check_response_body_containing_list_containing_subscription_elements,
            'Check Response Body Containing Number Of Entities':
                Checks.check_response_body_containing_number_of_entities,
            'Check Response Body Containing Context Source Registration element':
                Checks.check_response_body_containing_context_source_registration_element,
            'Check Response Body Containing EntityTypeList element':
                Checks.check_response_body_containing_entitytypelist_element,
            'Check Response Body Containing EntityType element':
                Checks.check_response_body_containing_entitytype_element,
            'Check Response Body Containing EntityTypeInfo element':
                Checks.check_response_body_containing_entitytypeinfo_element,
            'Check Response Body Containing AttributeList element':
                Checks.check_response_body_containing_attributelist_element,
            'Check Response Body Containing Attribute element':
                Checks.check_response_body_containing_attribute_element,
            'Check Response Body Containing List Containing Context Source Registrations elements':
                Checks.check_response_body_containing_list_containing_context_source_registrations_elements,
            'Check Response Body Containing ProblemDetails Element Containing Type Element set to':
                Checks.check_response_body_containing_problemdetails_element_containing_type_element_set_to,
            'Check Response Body Containing a Boolean Attribute set to':
                Checks.check_response_body_containing_a_boolean_attribute_set_to,
            'Check Response Body Contains DateTime Value':
                Checks.check_response_body_contains_datetime_value,
            'Check Response Body Containing ProblemDetails Element Containing Title Element':
                Checks.check_response_body_containing_problemdetails_element_containing_title_element,
            'Check Response Body Might Contain Additional Members of the NotificationParams':
                Checks.check_response_body_might_contain_additional_members_notification_params,
            'Check Response Body Might Contain Optional Fields':
                Checks.check_response_body_might_contain_optional_fields,
            'Check Response Body Containing ProblemDetails Element':
                Checks.check_response_body_containing_problemdetails_element,
            'Check JSON Value In Response Body':
                Checks.check_json_value_in_response_body,
            'Check Pagination Prev And Next Headers':
                Checks.check_pagination_prev_and_next_headers,
            'Check Created Resource Set To':
                Checks.check_created_resource_set_to,
            'Check Updated Resource Set To':
                Checks.check_updated_resource_set_to,
            'Check Updated Resources Set To':
                Checks.check_updated_resources_set_to,
            'Check SUT Not Containing Resource':
                Checks.check_sut_not_containing_resource,
            'Check SUT Not Containing Resources':
                Checks.check_sut_not_containing_resources,
            'Check NotificationParams':
                Checks.check_notificationparams,
            'Check Response Body Containing Batch Operation Result':
                Checks.check_response_body_containing_batch_operation_result,
            'Check Response Body Containing Update Result':
                Checks.check_response_body_containing_update_result,
            'Wait for notification':
                Checks.wait_for_notification,
            'Wait for no notification':
                Checks.wait_for_no_notification,
            'Wait for notification and validate it':
                Checks.wait_for_notification_and_validate_it,
            'Should be Equal':
                Checks.should_be_equal,
            'Dictionary Should Contain Key':
                Checks.dictionary_should_contain_key,
            'Dictionary Should Not Contain Key':
                Checks.dictionary_should_not_contain_key,
            'Should Not Be Empty':
                Checks.should_not_be_empty,
            'Should be True':
                Checks.should_be_true,
            'Should Be True':
                Checks.should_be_true,
            'Check Response Body Content':
                Checks.check_response_body_content,
            'Check JSON Value Not In Response Body':
                Checks.check_json_value_not_in_response_body,
            'Check Response Reason set to':
                Checks.check_response_reason_set_to,
            'Check Response Does Not Contain Body':
                Checks.check_response_does_not_contain_body,
            'Check Context Response Body Containing a list of identifiers':
                Checks.check_context_response_body_containing_a_list_of_identifiers,
            'Check Context Response Body Content':
                Checks.check_context_response_body_content,
            'Check Context Response Body Containing Detailed Information':
                Checks.check_context_response_body_containing_detailed_information,
            'Check Context Detailed Information Keys':
                Checks.check_context_detailed_information_keys,
            'Check Context Response Body Containing numberOfHits value':
                Checks.check_context_response_body_containing_numberofhits_value,
            'Check Response Kind set to':
                Checks.check_response_kind_set_to,
            'Check Cached @Contexts':
                Checks.check_cached_contexts,
            'Check Response Headers Link set to':
                Checks.check_response_headers_link_set_to,
            'Check Response Headers Containing NGSILD-Results-Count Equals To' :
                Checks.check_response_header_contains_ngsild_results_count_equals_to,
            'Check Messages Contain One Instance' :
                Checks.check_messages_contain_one_instance,
            'Check Message Contain Key':
                Checks.check_message_contain_key,
            'Check Message Field Equal':
                Checks.check_message_field_equal,
            'Check Content Range Part Equal':
                Checks.check_content_range_part_equal,
            'Check Data Is Empty':
                Checks.check_data_is_empty,
            'Check Notification Containing Entity Element':
                Checks.check_notification_containing_entity_element,
            'Check Notification Containing Entities Elements':
                Checks.check_notification_containing_entities_elements,
            'Check Body With Alternatives':
                Checks.check_body_with_alternatives
        }

        self.args = {
            'Check Response Status Code': {
                'params': ['status_code'],
                'position': [0]
            },
            'Check Response Body Is Empty': {
                'params': ['response_body'],
                'position': [0]
            },
            'Check Response Body Containing Array Of URIs set to': {
                'params': ['expected_entities_ids', 'response_body'],
                'position': [0, 1]
            },
            'Check Response Body Containing Entities URIS set to': {
                'params': ['expected_entities_ids', 'response_body'],
                'position': [0, 1]
            },
            'Check Response Body Containing ProblemDetails Element Containing Type Element set to': {
                'params': ['response_body', 'type'],
                'position': [0, 1]
            },
            'Check Response Body Containing ProblemDetails Element': {
                'params': ['problem_type'],
                'position': [1]
            },
            'Check Response Headers Containing Content-Type set to': {
                'params': ['content_type'],
                'position': [0]
            },
            'Check Updated Resource Set To': {
                'params': ['updated_resource', 'response_body', 'ignored_keys'],
                'position': []
            },
            'Check Response Body Containing ProblemDetails Element Containing Title Element': {
                'params': ['response_body'],
                'position': [0]
            },
            'Check Response Body Might Contain Additional Members of the NotificationParams': {
                'params': ['dictionary', 'key'],
                'position': [0, 1]
            },
            'Check Response Body Might Contain Optional Fields': {
                'params': ['dictionary', 'key'],
                'position': [0, 1]
            },
            'Check Response Headers Containing URI set to': {
                'params': ['expected_entity_id', 'response_headers'],
                'position': [0, 1]
            },
            'Check Response Body Containing EntityTypeInfo element': {
                'params': ['expectation_filename', 'response_body'],
                'position': [0, 1]
            },
            'Check Response Body Containing List Containing Entity Elements': {
                'params': ['expectation_filename', 'entities_ids', 'response_body', 'ignore_core_context_version'],
                'position': []
            },
            'Check Response Body Containing List Containing Entity Elements With Different Types': {
                'params': ['filename', 'entities_representation_ids', 'response_body', 'ignore_core_context_version'],
                'position': []
            },
            'Check Response Body Containing an Attribute set to': {
                'params': ['expected_attribute_name', 'response_body', 'expected_attribute_value'],
                'position': []
            },
            'Check Response Body Containing Attribute element': {
                'params': ['expectation_filename', 'response_body'],
                'position': [0, 1]
            },
            'Check Response Body Containing EntityTemporal element': {
                'params': ['filename', 'temporal_entity_representation_id', 'response_body'],
                'position': [0, 1, 2]
            },
            'Check SUT Not Containing Resources': {
                'params': ['response_body'],
                'position': [0]
            },
            'Check Response Body Containing List Containing EntityTemporal elements': {
                'params': ['filename', 'entity_ids'],
                'position': [0, 1]
            },
            'Check Response Body Containing a Boolean Attribute set to': {
                'params': ['expected_attribute_name', 'response_body', 'expected_attribute_value'],
                'position': [0, 1, 2]
            },
            'Check Response Body Contains DateTime Value': {
                'params': ['dictionary', 'key', 'expected value'],
                'position': [0, 1, 2]
            },
            'Check Response Body Containing List Containing Subscription elements': {
                'params': ['file', 'id', 'response'],
                'position': [0, 1, 2]
            },
            'Check Response Body Containing Number Of Entities': {
                'params': ['entity_type', 'number_entities', 'response'],
                'position': [0, 1, 2]
            },
            'Check Response Body Containing Context Source Registration element': {
                'params': ['file', 'id', 'response'],
                'position': [0, 1, 2]
            },
            'Check Response Body Containing EntityTypeList element': {
                'params': ['filename', 'response'],
                'position': [0, 1]
            },
            'Check Response Body Containing EntityType element': {
                'params': ['filename', 'response'],
                'position': [0, 1]
            },
            'Check Created Resource Set To': {
                'params': ['created_resource', 'response_body', 'ignored_keys'],
                'position': []
            },
            'Check Created Resources Set To': {
                'params': ['expected_resources', 'response_body', 'ignored_keys'],
                'position': []
            },
            'Check JSON Value In Response Body': {
                'params': ['key', 'value'],
                'position': [0, 1]
            },
            'Check Pagination Prev And Next Headers': {
                'params': ['previous', 'next'],
                'position': [1, 2]
            },
            'Check Updated Resources Set To': {
                'params': ['number_entities'],
                'position': [0]
            },
            'Check SUT Not Containing Resource': {
                'params': ['status_code'],
                'position': [0]
            },
            'Check NotificationParams': {
                'params': ['filename', 'expected_additional_members', 'response_body'],
                'position': [0, 1, 2]
            },
            'Check Response Body Containing Batch Operation Result': {
                'params': ['operation'],
                'position': [0]
            },
            'Check Response Body Containing Update Result': {
                'params': ['expected_update_result'],
                'position': [0]
            },
            'Should be Equal': {
                'params': ['expected_value', 'obtained_value'],
                'position': [0, 1]
            },
            'Check Response Body Containing Subscription element': {
                'params': ['filename', 'subscription_id', 'response_body', 'additional_ignored_keys'],
                'position': [0, 1, 2, 3]
            },
            'Wait for notification': {
                'params': ['timeout'],
                'position': []
            },
            'Wait for notification and validate it': {
                'params': ['expected_subscription_id', 'expected_context_source_registration_ids',
                           'expected_trigger_reason', 'expected_notification_data_entities',
                           'timeout'],
                'position': []
            },
            'Wait for no notification': {
                'params': ['timeout'],
                'position': []
            },
            'Check Response Body Containing AttributeList element': {
                'params': ['filename', 'response'],
                'position': [0, 1]
            },
            'Check Response Body Containing Entity element': {
                'params': ['filename', 'id', 'response'],
                'position': [0, 1, 2]
            },
            'Check Response Body Content': {
                'params': ['expectation_filename', 'response_body', 'additional_ignored_path'],
                'position': []
            },
            'Dictionary Should Contain Key': {
                'params': ['dictionary', 'key'],
                'position': [0, 1]
            },
            'Dictionary Should Not Contain Key': {
                'params': ['dictionary', 'key'],
                'position': [0, 1]
            },
            'Should Not Be Empty': {
                'params': ['variable'],
                'position': [0]
            },
            'Should be True': {
                'params': ['expression'],
                'position': [0]
            },
            'Should Be True': {
                'params': ['expression'],
                'position': [0]
            },
            'Check JSON Value Not In Response Body': {
                'params': ['json_path_expr'],
                'position': [0]
            },
            'Check Response Reason set to': {
                'params': ['reason'],
                'position': [1]
            },
            'Check Response Does Not Contain Body': {
                'params': ['response'],
                'position': [0]
            },
            'Check Context Response Body Containing a list of identifiers': {
                'params': ['response_body', 'expected_length', 'list_contexts', 'kind'],
                'position': []
            },
            'Check Context Response Body Content': {
                'params': ['expectation_filename', 'response_body'],
                'position': [0, 1]
            },
            'Check Context Response Body Containing Detailed Information': {
                'params': ['response_body', 'context_type'],
                'position': [0, 1]
            },
            'Check Context Detailed Information Keys': {
                'params': ['my_dict'],
                'position': [0]
            },
            'Check Context Response Body Containing numberOfHits value': {
                'params': ['response_body', 'expected_number_of_hists'],
                'position': [0, 1]
            },
            'Check Response Kind set to': {
                'params': ['response', 'kind'],
                'position': [0, 1]
            },
            'Check Cached @Contexts': {
                'params': ['context'],
                'position': [0]
            },
            'Check Response Headers Link set to': {
                'params': ['response_headers', 'expected_link_header'],
                'position': [0, 1]
            }
            ,
            'Check Response Headers Containing NGSILD-Results-Count Equals To': {
                'params': ['expected_result_count' , 'response_headers'],
                'position': [0, 1]
            },
            'Check Messages Contain One Instance': {
                'params': ['messages'],
                'position': [0]
            },
            'Check Message Contain Key': {
                'params': ['message', 'key'],
                'position': [0,1]
            },
            'Check Message Field Equal': {
                'params': ['expected_field', 'field'],
                'position': [0,1]
            },
            'Check Content Range Part Equal': {
                'params': ['expected_part' , 'part'],
                'position': [0, 1]
            },
            'Check Data Is Empty':{
                'params': ['data'],
                'position': [0]
            },
            'Check Notification Containing Entity Element': {
                'params': ['filename', 'notification'],
                'position': [0, 1]
            },
            'Check Notification Containing Entities Elements': {
                'params': ['filename', 'notification'],
                'position': [0, 1]
            },
            'Check Body With Alternatives': {
                'params': ['response_body', 'alternatives'],
                'position': [0, 1]
            }
        }

    @staticmethod
    def check_response_status_code(kwargs: list) -> str:
        if "status_code" in kwargs:
            status_code = kwargs['status_code']
            try:
                return f'Response Status Code set to {status_code} ({HTTPStatus(status_code).phrase})'
            except ValueError:
                return f'Response Status Code set to {status_code}'
        else:
            raise Exception(f'ERROR, Expected status_code parameter but received: {kwargs}')

    @staticmethod
    def wait_for_notification(kwargs: list) -> str:
        if 'timeout' in kwargs and kwargs['timeout'] != '':
            result = f"After waiting '{kwargs['timeout']}' seconds"
        else:
            result = f"After waiting '5' seconds"

        return result

    @staticmethod
    def wait_for_notification_and_validate_it(kwargs: list) -> str:
        expected_parameters = ['expected_subscription_id', 'expected_context_source_registration_ids',
                               'expected_trigger_reason', 'expected_notification_data_entities',
                               'timeout']

        if 'expected_notification_data_entities' not in kwargs:
            kwargs['expected_notification_data_entities'] = "${EMPTY}"

        if 'timeout' not in kwargs:
            kwargs['timeout'] = '5'

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Waiting for Notification and validate it"
        for key, value in kwargs.items():
            match key:
                case 'expected_subscription_id':
                    response = f"{response} and\n    Query Parameter: expected_subscription_id set to '{value}'"
                case 'expected_context_source_registration_ids':
                    response = f"{response} and\n    Query Parameter: expected_context_source_registration_ids set to '{value}'"
                case 'expected_trigger_reason':
                    response = f"{response} and\n    Query Parameter: expected_trigger_reason set to '{value}'"
                case 'expected_notification_data_entities':
                    response = f"{response} and\n    Query Parameter: expected_notification_data_entities set to '{value}'"
                case 'timeout':
                    response = f"{response} and\n    Query Parameter: timeout set to '{value}'"
                # If an exact match is not confirmed, this last case will be used if provided
                case 'checks':
                    pass
                case _:
                    raise Exception(f"ERROR, unexpected attribute '{result}', the attributes expected are "
                                    f"'{expected_parameters}', but received: {kwargs}")

        return response

    @staticmethod
    def check_response_headers_link_set_to(kwargs: list) -> str:
        return f"The response headers Link is set to '{kwargs['expected_link_header']}'"

    @staticmethod
    def wait_for_no_notification(kwargs: list) -> str:
        expected_parameters = ['timeout']

        if 'timeout' not in kwargs:
            kwargs['timeout'] = '5'

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Waiting for no Notification data"
        for key, value in kwargs.items():
            match key:
                case 'timeout':
                    response = f"{response} and\n    Query Parameter: timeout set to '{value}'"
                # If an exact match is not confirmed, this last case will be used if provided
                case 'checks':
                    pass
                case _:
                    raise Exception(f"ERROR, unexpected attribute '{result}', the attributes expected are "
                                    f"'{expected_parameters}', but received: {kwargs}")

        return response

    @staticmethod
    def check_response_reason_set_to(kwargs: list) -> str:
        return f"Response reason set to '{kwargs['reason']}'"

    @staticmethod
    def check_response_does_not_contain_body(kwargs: list) -> str:
        return f"Response does not contain a body"

    @staticmethod
    def check_response_body_is_empty(kwargs: list) -> str:
        return 'Response Body is empty'

    @staticmethod
    def check_response_body_containing_array_of_uris_set_to(kwargs: list) -> str:
        return 'Response Body set to an array of created entities ids'

    @staticmethod
    def check_response_body_containing_entities_uris_set_to(kwargs: list) -> str:
        return 'Response Body contains entities ids'

    @staticmethod
    def check_created_resources_set_to(kwargs: list) -> str:
        return 'Created resources set to ${entities}'

    @staticmethod
    def dictionary_should_contain_key(kwargs: list) -> str:
        if 'dictionary' in kwargs and 'key' in kwargs:
            return f"The dictionary `{kwargs['dictionary']}' should contain the key '{kwargs['key']}'"
        else:
            raise Exception(f"ERROR, Expected 'dictionary' and 'key' parameters but received: {kwargs}")
    
    @staticmethod
    def dictionary_should_not_contain_key(kwargs: list) -> str:
        if 'dictionary' in kwargs and 'key' in kwargs:
            return f"The dictionary `{kwargs['dictionary']}' should not contain the key '{kwargs['key']}'"
        else:
            raise Exception(f"ERROR, Expected 'dictionary' and 'key' parameters but received: {kwargs}")

    @staticmethod
    def check_context_response_body_content(kwargs: list) -> str:
        if 'expectation_filename' in kwargs and 'response_body' in kwargs:
            return f"Check the Body of the response should contain the @context `{kwargs['expectation_filename']}'"
        else:
            raise Exception(f"ERROR, Expected 'dictionary' and 'key' parameters but received: {kwargs}")

    @staticmethod
    def check_context_response_body_containing_detailed_information(kwargs: list) -> str:
        if 'response_body' in kwargs and 'context_type' in kwargs:
            return (f"Check the Body of the response should contain a @context "
                    f"with 'URL' key not Empty and 'string' type, "
                    f"with 'localId' key not Empty and 'string' type, "
                    f"with 'kind' key not Empty, 'string' type, and value set to '{kwargs['context_type']}', "
                    f"and 'timestamp and 'DateTime' format")
        else:
            raise Exception(f"ERROR, Expected 'response_body' and 'context_type' parameters but received: {kwargs}")

    @staticmethod
    def check_response_body_might_contain_additional_members_notification_params(kwargs: list) -> str:
        if 'dictionary' in kwargs and 'key' in kwargs:
            return f"The Response Body `{kwargs['dictionary']}' might contain the key '{kwargs['key']}' in the NotificationParams"
        else:
            raise Exception(f"ERROR, Expected 'dictionary' and 'key' parameters but received: {kwargs}")

    @staticmethod
    def check_response_body_might_contain_optional_fields(kwargs: list) -> str:
        if 'dictionary' in kwargs and 'key' in kwargs:
            return f"The Response Body `{kwargs['dictionary']}' might contain the key '{kwargs['key']}'"
        else:
            raise Exception(f"ERROR, Expected 'dictionary' and 'key' parameters but received: {kwargs}")

    @staticmethod
    def should_not_be_empty(kwargs: list) -> str:
        if 'variable' in kwargs:
            return f"The variable `{kwargs['variable']}' should not be '${{EMPTY}}'"
        else:
            raise Exception(f'ERROR, Expected dictionary and key parameters but received: {kwargs}')

    @staticmethod
    def should_be_true(kwargs: list) -> str:
        if 'expression' in kwargs:
            return f"The expression `{kwargs['expression']}' should be True"
        else:
            raise Exception(f'ERROR, Expected dictionary and key parameters but received: {kwargs}')

    @staticmethod
    def check_response_headers_containing_content_type_set_to(kwargs: list) -> str:
        if "content_type" in kwargs:
            content_type = kwargs['content_type']
            return f'Response Header: Content-Type set to {content_type}'
        else:
            raise Exception(f'ERROR, Expected status_code parameter but received: {kwargs}')

    @staticmethod
    def check_response_body_containing_a_boolean_attribute_set_to(kwargs: list) -> str:
        return (f"Check that the payload body contains a boolean attribute with name "
                f"'{kwargs['expected_attribute_name']}' and value '{kwargs['expected_attribute_value']}")

    @staticmethod
    def check_response_headers_link_not_empty(kwargs: list) -> str:
        return f'Response Header: Link is not Empty'

    @staticmethod
    def check_response_headers_containing_uri_set_to(kwargs: list) -> str:
        if 'expected_entity_id' in kwargs and 'response_headers' in kwargs:
            return f"Response Header: Location containing ${kwargs['expected_entity_id']}"
        else:
            raise Exception(f'ERROR, Expected expected_entity_id and response_headers parameters '
                            f'but received: {kwargs}')

    @staticmethod
    def check_response_headers_id_not_empty(kwargs: list) -> str:
        return 'Response Header: Location is not Empty'

    @staticmethod
    def check_response_body_containing_an_attribute_set_to(kwargs: list) -> str:
        expected_parameters = ['checks', 'expected_attribute_name', 'response_body', 'expected_attribute_value']

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Check Response Body containing an Attribute set to"
        for key, value in kwargs.items():
            match key:
                case 'expected_attribute_name':
                    response = f"{response} and\n    Query Parameter: expected_attribute_name set to '{value}'"
                case 'response_body':
                    response = f"{response} and\n    Query Parameter: response_body set to '{value}'"
                case 'expected_attribute_value':
                    response = f"{response} and\n    Query Parameter: expected_attribute_value set to '{value}'"
                # If an exact match is not confirmed, this last case will be used if provided
                case 'checks':
                    pass
                case _:
                    raise Exception(f"ERROR, unexpected attribute '{result}', the attributes expected are "
                                    f"'{expected_parameters}', but received: {kwargs}")

        return response

    @staticmethod
    def check_context_response_body_containing_a_list_of_identifiers(kwargs: list) -> str:
        expected_parameters = ['response_body', 'expected_length', 'list_contexts', 'kind']

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Check Context Response Body Containing a list of identifiers"
        for key, value in kwargs.items():
            match key:
                case 'response_body':
                    response = f"{response} and\n    Query Parameter: {key} set to '{value}'"
                case 'expected_length':
                    response = f"{response} and\n    Query Parameter: {key} set to '{value}'"
                case 'list_contexts':
                    response = f"{response} and\n    Query Parameter: {key} set to '{value}'"
                case 'kind':
                    response = f"{response} and\n    Query Parameter: {key} set to '{value}'"
                case 'checks':
                    pass
                case _:
                    raise Exception(f"ERROR, unexpected attribute '{result}', the attributes expected are "
                                    f"'{expected_parameters}', but received: {kwargs}")

        return response

    @staticmethod
    def check_context_detailed_information_keys(kwargs: list) -> str:
        return (f"Check that the only allowed keys in the response body of a @context are 'URL', 'localId', 'kind', "
                f"'timestamp', 'lastUsage', 'numberOfHits', 'extraInfo'")

    @staticmethod
    def check_context_response_body_containing_numberofhits_value(kwargs: list) -> str:
        return f"Check that the numberOfHits in the response body is set to '{kwargs['expected_number_of_hists']}'"

    @staticmethod
    def check_response_kind_set_to(kwargs: list) -> str:
        return f"Check that the Kind of the @context is set to '{kwargs['kind']}'"

    @staticmethod
    def check_cached_contexts(kwargs: list) -> str:
        return f"Check that for each @context in the response body, they are of the type 'Cached'"

    @staticmethod
    def check_response_body_containing_entity_element(kwargs: list) -> str:
        if 'filename' in kwargs and 'id' in kwargs and 'response' in kwargs:
            return f"Response Body containing en entity element with id set to '{kwargs['id']}' and body content set to '{kwargs['filename']}'"

    @staticmethod
    def check_response_body_containing_list_containing_entity_elements(kwargs: list) -> str:
        expected_parameters = ['expectation_filename', 'entities_ids', 'response_body', 'ignore_core_context_version']

        if 'ignore_core_context_version' not in kwargs:
            kwargs['ignore_core_context_version'] = False

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Response Body containing a list containing Entity Elements"
        for key, value in kwargs.items():
            match key:
                case 'expectation_filename':
                    response = f"{response} and\n    Query Parameter: {key} set to '{value}'"
                case 'entities_ids':
                    response = f"{response} and\n    Query Parameter: {key} set to '{value}'"
                case 'response_body':
                    response = f"{response} and\n    Query Parameter: {key} set to '{value}'"
                case 'ignore_core_context_version':
                    response = f"{response} and\n    Query Parameter: {key} set to '{value}'"
                case 'checks':
                    pass
                case _:
                    raise Exception(f"ERROR, unexpected attribute '{result}', the attributes expected are "
                                    f"'{expected_parameters}', but received: {kwargs}")

        return response

    @staticmethod
    def check_response_body_containing_list_containing_entity_elements_with_different_types(kwargs: list) -> str:
        expected_parameters = ['filename', 'entities_representation_ids', 'response_body', 'ignore_core_context_version']

        if 'ignore_core_context_version' not in kwargs:
            kwargs['ignore_core_context_version'] = False

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Response body containing a list containing entity elements with different types"
        for key, value in kwargs.items():
            match key:
                case 'filename':
                    response = f"{response} and\n    Query Parameter: {key} set to '{value}'"
                case 'entities_representation_ids':
                    response = f"{response} and\n    Query Parameter: {key} set to '{value}'"
                case 'response_body':
                    response = f"{response} and\n    Query Parameter: {key} set to '{value}'"
                case 'ignore_core_context_version':
                    response = f"{response} and\n    Query Parameter: {key} set to '{value}'"
                case 'checks':
                    pass
                case _:
                    raise Exception(f"ERROR, unexpected attribute '{result}', the attributes expected are "
                                    f"'{expected_parameters}', but received: {kwargs}")

        return response

    @staticmethod
    def check_response_body_containing_entitytemporal_element(kwargs: list) -> str:
        if 'filename' in kwargs and 'temporal_entity_representation_id' in kwargs and 'response_body' in kwargs:
            return (f"Response Body containing EntityTemporal element containing attribute instances in the time range"
                    f" specified by the NGSI-LD temporal query:\n"
                    f"    * the payload is defined in the file set to '{kwargs['filename']}'\n"
                    f"    * the id was changed to '{kwargs['temporal_entity_representation_id']}'\n"
                    f"    * response body to be checked set to '{kwargs['response_body']}'")
        else:
            raise Exception(f"ERROR, expected 'filename', 'temporal_entity_representation_id', and 'response_body' "
                            f"attributes, received: '{kwargs}'")

    @staticmethod
    def check_response_body_containing_list_containing_entitytemporal_elements(kwargs: list) -> str:
        if 'filename' in kwargs and 'entity_ids' in kwargs:
            return (f"Request response body containing a list that contains Entity Temporal Elements\n"
                    f"    compared with file '{kwargs['filename']}'\n"
                    f"    and using the list of entity ids define in '{kwargs['entity_ids']}'")
        else:
            raise Exception(f"ERROR, expected parameters 'filename' and 'entity_ids', but received '{kwargs}'")

    @staticmethod
    def check_response_body_contains_datetime_value(kwargs: list) -> str:
        # ${dictionary}    ${key}    ${expected value}
        return (f"Check that the dictionary contains the key '{kwargs['key']}' with type DateTime and value set "
                f"to '{kwargs['expected value']}'")

    @staticmethod
    def check_response_body_containing_subscription_element(kwargs: list) -> str:
        if 'filename' in kwargs and 'subscription_id' in kwargs and 'response_body' in kwargs:
            return (f"Response Body containing the same content defined in file '{kwargs['filename']}'"
                    f" with subscription id '{kwargs['subscription_id']}'")
        else:
            raise Exception(f"ERROR, expected parameters 'filename' and 'entity_ids', but received '{kwargs}'")

    @staticmethod
    def check_response_body_containing_list_containing_subscription_elements(kwargs: list) -> str:
        if 'file' in kwargs and 'id' in kwargs and 'response' in kwargs:
            return (f"Response containing:\n"
                    f"    * file set to '{kwargs['file']}'\n"
                    f"    * id set to '{kwargs['id']}'\n"
                    f"    * response set to '{kwargs['response']}'")
        else:
            raise Exception(f"ERROR, expected 'file', 'id', and 'response' attributes, received: '{kwargs}'")

    @staticmethod
    def check_response_body_containing_number_of_entities(kwargs: list) -> str:
        if "entity_type" in kwargs and 'number_entities' in kwargs and 'response' in kwargs:
            number_entities = kwargs['number_entities']
            entity_type = kwargs['entity_type']
            response = kwargs['response']
            return (f"Response Body containing a list of entities equal to '{number_entities}' of type '{entity_type}' "
                    f"with response set to '{response}'")
        else:
            raise Exception(f'ERROR, expected entity_type and number_entities attributes, but received: {kwargs}')

    @staticmethod
    def check_response_body_containing_context_source_registration_element(kwargs: list) -> str:
        if 'file' in kwargs and 'id' in kwargs and 'response' in kwargs:
            return (f"Response containing:\n"
                    f"    * file set to '{kwargs['file']}'\n"
                    f"    * id set to '{kwargs['id']}'\n"
                    f"    * response set to '{kwargs['response']}'")
        else:
            raise Exception(f"ERROR, expected 'file', 'id', and 'response' attributes, received: '{kwargs}'")
        # if 'csr_description' in kwargs:
        #     csr = kwargs['csr_description']
        #     return f"Response body containing a '{csr}'"
        # else:
        #     raise Exception(f"ERROR, expected csr_description attribute, but received: {kwargs}")

    @staticmethod
    def check_response_body_containing_entitytypelist_element(kwargs: list) -> str:
        if 'filename' in kwargs and 'response' in kwargs:
            return f"Response Body containing an Entity Type List with expectation body equal to file: '{kwargs['filename']}'"
        else:
            raise Exception(f"ERROR, expected filename and response attributes, but received: {kwargs}")

    @staticmethod
    def check_response_body_containing_entitytype_element(kwargs: list) -> str:
        if 'filename' in kwargs and 'response' in kwargs:
            return f"Response Body containing an Entity Type Element with expectation body equal to file: '{kwargs['filename']}'"
        else:
            raise Exception(f"ERROR, expected filename and response attributes, but received: {kwargs}")

    @staticmethod
    def check_response_body_containing_entitytypeinfo_element(kwargs: list) -> str:
        if 'expectation_filename' in kwargs and 'response_body' in kwargs:
            return (f"Response body containing an Entity Type Info "
                    f"with expectation body set to file '{kwargs['expectation_filename']}' "
                    f"and response body to be checked set to '{kwargs['response_body']}'")
        else:
            raise Exception(f"ERROR, expected filename and response attributes, but received: {kwargs}")

    @staticmethod
    def check_response_body_containing_attributelist_element(kwargs: list) -> str:
        if 'filename' in kwargs and 'response' in kwargs:
            return (f"Response Body containing an Attribute List element"
                    f"\n    * with filename set to '{kwargs['filename']}'"
                    f"\n    * response set to '{kwargs['response']}'")

    @staticmethod
    def check_response_body_containing_attribute_element(kwargs: list) -> str:
        if 'expectation_filename' in kwargs and 'response_body' in kwargs:
            return (f"Response body containing an array of attributes"
                    f"\n    * with the expected payload defined in the file '{kwargs['expectation_filename']}'"
                    f"\n    * and response body set to '{kwargs['response_body']}'")

    @staticmethod
    def check_response_body_containing_list_containing_context_source_registrations_elements(kwargs: list) -> str:
        return 'Response body set to list of all matching Context Source Registrations resolved against the default JSON-LD context'

    @staticmethod
    def check_response_body_containing_problemdetails_element_containing_type_element_set_to(kwargs: list) -> str:
        if 'type' in kwargs:
            type = kwargs['type']
            return f"Response Body containing the type '{type}'"
        else:
            raise Exception(f"ERROR, expected type attribute, but received: {kwargs}")

    @staticmethod
    def check_response_body_containing_problemdetails_element(kwargs: list) -> str:
        if 'problem_type' in kwargs:
            problem_type = kwargs['problem_type']
            return (f"Response Body containing the type '{problem_type}' and "
                    f"Response Body Title is a string and is not ${{EMPTY}} and "
                    f"Response Body Detail is a string and is not ${{EMPTY}} and "
                    f"Response Boty Title is not equal to Response Body Detail")
        else:
            raise Exception(f"ERROR, expected type attribute, but received: {kwargs}")

    @staticmethod
    def check_response_body_containing_problemdetails_element_containing_title_element(kwargs: list) -> str:
        return "Response body containing 'title' element"

    @staticmethod
    def check_json_value_in_response_body(kwargs: list) -> str:
        if 'key' in kwargs and 'value' in kwargs:
            key = kwargs['key']
            value = kwargs['value']
            return f"Response Body containing the key '{key}', with the value '{value}'"
        else:
            raise Exception(f"ERROR, expected key and value attributes, but received: {kwargs}")

    @staticmethod
    def check_json_value_not_in_response_body(kwargs: list) -> str:
        if 'json_path_expr' in kwargs:
            key = kwargs['json_path_expr']
            return f"Check that response body does not contain the key '{key}'"
        else:
            raise Exception(f"ERROR, expected key and value attributes, but received: {kwargs}")

    @staticmethod
    def check_pagination_prev_and_next_headers(kwargs: list) -> str:
        previous = None
        next = None
        if 'previous' in kwargs:
            previous = kwargs['previous']
            previous_text = f"with 'Prev' header equal to '{previous}'"

        if 'next' in kwargs:
            next = kwargs['next']
            next_text = f"with 'Next' header equal to '{next}'"

        if previous is None and next is None:
            raise Exception(f"ERROR, expected previous or next attributes, but received: {kwargs}")
        elif previous is not None and next is None:
            result = f"Response header {previous_text}"
        elif previous is None and next is not None:
            result = f"Response header {next_text}"
        else:
            result = f"Response header {previous_text} and {next_text}"

        return result

    @staticmethod
    def check_created_resource_set_to(kwargs: list) -> str:
        expected_parameters = ['created_resource', 'response_body', 'ignored_keys']

        if 'ignored_keys' not in kwargs:
            kwargs['ignored_keys'] = '${None}'

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Check Created Resource Set To"
        for key, value in kwargs.items():
            match key:
                case 'created_resource':
                    response = f"{response} and\n    Query Parameter: '{key}' set to '{value}'"
                case 'response_body':
                    response = f"{response} and\n    Query Parameter: '{key}' set to '{value}'"
                case 'ignored_keys':
                    response = f"{response} and\n    Query Parameter: '{key}' set to '{value}' list of keys"
                # If an exact match is not confirmed, this last case will be used if provided
                case 'checks':
                    pass
                case _:
                    raise Exception(f"ERROR, unexpected attribute '{result}', the attributes expected are "
                                    f"'{expected_parameters}', but received: {kwargs}")

        return response

    @staticmethod
    def check_created_resources_set_to(kwargs: list) -> str:
        expected_parameters = ['expected_resources', 'response_body', 'ignored_keys']

        if 'ignored_keys' not in kwargs:
            kwargs['ignored_keys'] = '${None}'

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Check Created Resource Set To"
        for key, value in kwargs.items():
            match key:
                case 'expected_resources':
                    response = f"{response} and\n    Query Parameter: '{key}' set to '{value}'"
                case 'response_body':
                    response = f"{response} and\n    Query Parameter: '{key}' set to '{value}'"
                case 'ignored_keys':
                    response = f"{response} and\n    Query Parameter: '{key}' set to '{value}'"
                # If an exact match is not confirmed, this last case will be used if provided
                case 'checks':
                    pass
                case _:
                    raise Exception(f"ERROR, unexpected attribute '{result}', the attributes expected are "
                                    f"'{expected_parameters}', but received: {kwargs}")

        return response

    @staticmethod
    def check_updated_resource_set_to(kwargs: list) -> str:
        # return "Updated Entity set to ${entity}"
        expected_parameters = ['updated_resource', 'response_body', 'ignored_keys']

        if 'ignored_keys' not in kwargs:
            kwargs['ignored_keys'] = '${None}'

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Check Updated Entity"
        for key, value in kwargs.items():
            match key:
                case 'updated_resource':
                    response = f"{response} and\n    Query Parameter: '{key}' set to '{value}'"
                case 'response_body':
                    response = f"{response} and\n    Query Parameter: '{key}' set to '{value}'"
                case 'ignored_keys':
                    response = f"{response} and\n    Query Parameter: '{key}' set to '{value}'"
                # If an exact match is not confirmed, this last case will be used if provided
                case 'checks':
                    pass
                case _:
                    raise Exception(f"ERROR, unexpected attribute '{result}', the attributes expected are "
                                    f"'{expected_parameters}', but received: {kwargs}")

        return response

    @staticmethod
    def check_updated_resources_set_to(kwargs: list) -> str:
        if 'number_entities' in kwargs:
            number_entities = kwargs['number_entities']
            return f"Updated Entities set to '{number_entities}' valid entities"

    @staticmethod
    def check_sut_not_containing_resource(kwargs: list) -> str:
        if "status_code" in kwargs:
            status_code = kwargs['status_code']
            try:
                return f'Response Status Code set to {status_code} ({HTTPStatus(status_code).phrase})'
            except ValueError:
                return f'Response Status Code set to {status_code}'
        else:
            raise Exception(f'ERROR, Expected status_code parameter but received: {kwargs}')

    @staticmethod
    def check_sut_not_containing_resources(kwargs: list) -> str:
        return f'Response body is empty'

    @staticmethod
    def check_notificationparams(kwargs: list) -> str:
        if 'filename' in kwargs and 'expected_additional_members' in kwargs and 'response_body' in kwargs:
            return (f"Response containing:\n"
                    f"    * Notification expectation file path set to '{kwargs['filename']}'\n"
                    f"    * Expected Additional Members set to '{kwargs['expected_additional_members']}'\n"
                    f"    * Response Body set to '{kwargs['response_body']}'\n")
        else:
            raise Exception(f"ERROR, expected 'filename', 'expected_additional_members', and 'response_body', "
                            f"but received '{kwargs}'")

    @staticmethod
    def check_response_body_containing_batch_operation_result(kwargs: list) -> str:
        if "operation" in kwargs:
            return f"Response body containing batch operation result set to '{kwargs['operation']}'"
        else:
            raise Exception(f'ERROR, Expected operation parameter but received: {kwargs}')

    @staticmethod
    def check_response_body_containing_update_result(kwargs: list) -> str:
        if "expected_update_result" in kwargs:
            return f"Response body containing update result set to '{kwargs['expected_update_result']}'"
        else:
            raise Exception(f'ERROR, Expected expected_update_result parameter but received: {kwargs}')

    @staticmethod
    def check_response_body_content(kwargs: list) -> str:
        expected_parameters = ['expectation_filename', 'response_body', 'additional_ignored_path']

        if 'additional_ignored_path' not in kwargs:
            kwargs['additional_ignored_path'] = '${EMPTY}'

        result = [x for x in kwargs if x not in expected_parameters]
        response = "Check Response Body Content"
        for key, value in kwargs.items():
            match key:
                case 'expectation_filename':
                    response = f"{response} and\n    Query Parameter: expectation_filename set to '{value}'"
                case 'response_body':
                    response = f"{response} and\n    Query Parameter: response_body set to '{value}'"
                case 'additional_ignored_path':
                    response = f"{response} and\n    Query Parameter: additional_ignored_path set to '{value}'"
                # If an exact match is not confirmed, this last case will be used if provided
                case 'checks':
                    pass
                case _:
                    raise Exception(f"ERROR, unexpected attribute '{result}', the attributes expected are "
                                    f"'{expected_parameters}', but received: {kwargs}")

        return response


    @staticmethod
    def should_be_equal(kwargs: list) -> str:
        if 'expected_value' in kwargs and 'obtained_value' in kwargs:
            return f"Notification data: '{kwargs['obtained_value']}' equal to '{kwargs['expected_value']}'"
        else:
            raise Exception(f"ERROR, Expected 'expected_value' and 'obtained_value' parameters but received: '{kwargs}'")

    @staticmethod
    def check_response_header_contains_ngsild_results_count_equals_to(kwargs: list) -> str:
        if 'expected_result_count' in kwargs:
            expected_result_count = kwargs['expected_result_count']
            return f'Response Header: NGSILD-Results-Count equals to {expected_result_count}'
        else:
            raise Exception(f"ERROR, Expected 'expected_result_count' but received: '{kwargs}'")

    @staticmethod
    def check_messages_contain_one_instance(kwargs: list) -> str:
        if 'messages' in kwargs:
            messages = kwargs['messages']
            return f'Received messages {messages}'
        else:
            raise Exception(f"ERROR, Expected 1 message but received: '{kwargs}'")

    @staticmethod
    def check_message_contain_key(kwargs: list) -> str:
        if 'message' in kwargs and 'key' in kwargs:
            message = kwargs['message']
            key = kwargs['key']
            return f'Received message {message} with key {key}'
        else:
            raise Exception(f"ERROR, Expected a message containing the key but received: '{kwargs}'")

    @staticmethod
    def check_message_field_equal(kwargs: list) -> str:
        if 'expected_field' in kwargs and 'field' in kwargs:
            field = kwargs['field']
            expected_field = kwargs['expected_field']
            return f'expected field {expected_field} equals field {field}'
        else:
            raise Exception(f"ERROR, Expected the field to be equal 'expected_field' but received: '{kwargs}'")

    @staticmethod
    def check_notification_containing_entity_element(kwargs: list) -> str:
        if 'filename' in kwargs and 'notification' in kwargs:
            return f"Notification containing entity element set to '{kwargs['filename']}'"
        else:
            raise Exception(f"ERROR, Expected 'filename' and 'notification' but received: '{kwargs}'")

    @staticmethod
    def check_notification_containing_entities_elements(kwargs: list) -> str:
        if 'filename' in kwargs and 'notification' in kwargs:
            return f"Notification containing entities elements set to '{kwargs['filename']}'"
        else:
            raise Exception(f"ERROR, Expected 'filename' and 'notification' but received: '{kwargs}'")

    @staticmethod
    def check_content_range_part_equal(kwargs: list) -> str:
        if 'expected_part' in kwargs and 'part' in kwargs:
            expected_part = kwargs['expected_part']
            part = kwargs['part']
            return f'Content range part {part} equals {expected_part}'
        else:
            raise Exception(f"ERROR, Expected 'expected_part' but received: '{kwargs}'")

    @staticmethod
    def check_data_is_empty(kwargs: list) -> str:
        if 'data' in kwargs :
            data = kwargs['data']
            return f'data is empty : {data}'
        else:
            raise Exception(f"ERROR, Expected empty data but received: '{kwargs}'")

    @staticmethod
    def check_body_with_alternatives(kwargs: list) -> str:
        if 'response_body' in kwargs and 'alternatives' in kwargs :
            response_body = kwargs['response_body']
            alternatives = kwargs['alternatives']
            return f'response body : {response_body} found in alternatives : {alternatives}'
        else:
            raise Exception(f"ERROR, Expected 'response_body' to be present in 'alternatives' but received: '{kwargs}'")


    def get_checks(self, **kwargs) -> str:
        checking = None

        if "checks" in kwargs:
            checking = kwargs["checks"]
        else:
            raise Exception(f'ERROR,  the attribute checks is mandatory, but received: {kwargs}')

        if isinstance(checking, str):
            result = self.checks[checking](kwargs)
        elif isinstance(checking, list):
            result = [self.checks[x](kwargs) for x in checking]
            result = " and\n".join(result)
        else:
            raise Exception(f"ERROR, checks type not supported: {checking}")

        return result


if __name__ == "__main__":
    data = Checks()

    print(data.get_checks(checks='Check Response Status Code',
                          status_code=201))
    print(data.get_checks(checks='Check Response Body Is Empty'))
    print(data.get_checks(checks='Check Response Body Containing Array Of URIs set to'))
    print(data.get_checks(checks='Check Response Body Containing Entities URIS set to'))
    print(data.get_checks(checks='Check Created Resources Set To'))
    print(data.get_checks(checks='Check Response Headers Containing Content-Type set to',
                          content_type='application/json'))
    print(data.get_checks(checks='Check Response Headers Link Not Empty'))
    print(data.get_checks(checks='Check Response Headers Containing URI set to'))
    print(data.get_checks(checks='Check Response Headers ID Not Empty'))
    print(data.get_checks(checks='Check Response Body Containing an Attribute set to',
                          attribute_name='status'))
    print(data.get_checks(checks='Check Response Body Containing an Attribute set to',
                          attribute_name='status',
                          attribute_value='active'))
    print(data.get_checks(checks='Check Response Body Containing Entity element'))
    print(data.get_checks(checks='Check Response Body Containing List Containing Entity Elements'))
    print(data.get_checks(checks='Check Response Body Containing List Containing Entity Elements With Different Types'))
    print(data.get_checks(checks='Check Response Body Containing EntityTemporal element'))
    print(data.get_checks(checks='Check Response Body Containing List Containing EntityTemporal elements',
                          timeRel='after',
                          timeAt='2020-07-01T12:05:00Z'))
    print(data.get_checks(checks='Check Response Body Containing Subscription element'))
    print(data.get_checks(checks='Check Response Body Containing List Containing Subscription elements',
                          number=2))
    print(data.get_checks(checks='Check Response Body Containing List Containing Subscription elements',
                          number=1))
    print(data.get_checks(checks='Check Response Body Containing Number Of Entities',
                          entity_type="Vehicle",
                          number_entities=3))
    print(data.get_checks(checks='Check Response Body Containing Context Source Registration element',
                          csr_description='Context Source Registration'))
    print(data.get_checks(checks='Check Response Body Containing EntityTypeList element',
                          description='Json object with list of entity types with context'))
    print(data.get_checks(checks='Check Response Body Containing EntityType element',
                          description='Json object with an entity type with context'))
    print(data.get_checks(checks='Check Response Body Containing EntityTypeInfo element'))
    print(data.get_checks(checks='Check Response Body Containing AttributeList element'))
    print(data.get_checks(checks='Check Response Body Containing Attribute element'))
    print(data.get_checks(checks='Check Response Body Containing List Containing Context Source Registrations elements'))
    print(data.get_checks(checks='Check Response Body Containing ProblemDetails Element Containing Type Element set to',
                          type='https://uri.etsi.org/ngsi-ld/errors/BadRequestData'))
    print(data.get_checks(checks='Check Response Body Containing ProblemDetails Element Containing Title Element'))
    print(data.get_checks(checks='Check JSON Value In Response Body',
                          key="['information']['entities'][0]['type']",
                          value="Building"))
    print(data.get_checks(checks='Check Pagination Prev And Next Headers',
                          previous='</ngsi-ld/v1/csourceSubscriptions?limit=1&page=1>;rel="prev";type="application/ld+json"',
                          next='</ngsi-ld/v1/csourceSubscriptions?limit=1&page=3>;rel="next";type="application/ld+json"'))
    print(data.get_checks(checks='Check Pagination Prev And Next Headers',
                          previous='',
                          next='</ngsi-ld/v1/csourceSubscriptions?limit=1&page=3>;rel="next";type="application/ld+json"'))
    print(data.get_checks(checks='Check Pagination Prev And Next Headers',
                          previous='</ngsi-ld/v1/csourceSubscriptions?limit=1&page=1>;rel="prev";type="application/ld+json"',
                          next=''))
    print(data.get_checks(checks='Check Pagination Prev And Next Headers',
                          previous='',
                          next=''))
    print(data.get_checks(checks='Check Created Resource Set To'))
    print(data.get_checks(checks='Check Updated Resource Set To'))
    print(data.get_checks(checks='Check Updated Resources Set To',
                          number_entities=2))
    print(data.get_checks(checks='Check SUT Not Containing Resource',
                          status_code=404))
    print(data.get_checks(checks='Check SUT Not Containing Resources'))
    print(data.get_checks(checks='Check NotificationParams',
                          format="keyValues",
                          uri="http://localhost:1111/notify",
                          accept="application/json",
                          status="ok",
                          timesSent="1"))

    print()
    print(data.get_checks(checks=
                          ['Check Response Status Code',
                           'Check Response Body Containing Array Of URIs set to',
                           'Check Created Resources Set To']
                          , status_code=201))
    print()
    print(data.get_checks(checks='Check Response Headers Containing NGSILD-Results-Count Equals To'))

    print()
    print(data.get_checks(checks='Check Notification Containing Entity Element'))
    print()
    print(data.get_checks(checks='Check Body With Alternatives'))

    # Check exceptions
    try:
        print(data.get_checks(checks='Check Response Body Containing an Attribute set to'))
    except Exception as e:
        print(e)
