*** Settings ***
Documentation       Check that one can replace an existing entity attribute and that its createdAt Temporal Property remains unchanged

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Initial Entity
Test Template       Replace Entity Attribute


*** Variables ***
${entity_filename}      vehicle-speed-multi-instances.jsonld


*** Test Cases ***    ATTRIBUTE_FILE_NAME    EXPECTATION_FILE_NAME
055_01_01 Replace Entity Attribute
    [Tags]    ea-replace    5_6_19    6_7_3_3    since_v1.6.1
    vehicle-isParked-attribute.json    vehicle-replace-isParked-attribute.jsonld
055_01_02 Replace Entity Attribute Giving A New Attribute Type
    [Tags]    ea-replace    5_6_19    6_7_3_3    since_v1.6.1
    vehicle-isParked-new-type-attribute.json    vehicle-replace-isParked-new-type-attribute.jsonld


*** Keywords ***
Replace Entity Attribute
    [Documentation]    Check that one can replace an existing entity attribute and that its createdAt Temporal Property remains unchanged
    [Arguments]    ${attribute_replacement_filename}    ${expectation_filename}
    ${attribute}=    Load Test Sample
    ...    test_sample_file_path=entities/${attribute_replacement_filename}
    ...    test_sample_id=${entity_id}
    ${response}=    Replace Attribute Selecting Content Type
    ...    entity_id=${entity_id}
    ...    attr_id=isParked
    ...    attribute_fragment=${attribute}
    ...    content_type=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${response1}=    Retrieve Entity
    ...    id=${entity_id}
    ...    context=${ngsild_test_suite_context}
    ...    options=sysAttrs
    ${ignored_attributes}=    Create List    @context    createdAt    modifiedAt
    ${entity_expectation_payload}=    Load Test Sample    entities/expectations/${expectation_filename}    ${entity_id}
    Check Updated Resource Set To
    ...    updated_resource=${entity_expectation_payload}
    ...    response_body=${response1.json()}
    ...    ignored_keys=${ignored_attributes}
    Check JSON Value In Response Body
    ...    json_path_expr=['isParked']['createdAt']
    ...    value_to_check=${createdAt}
    ...    response_body=${response1.json()}

Setup Initial Entity
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${entity_filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity
    ...    id=${entity_id}
    ...    context=${ngsild_test_suite_context}
    ...    options=sysAttrs
    ${createdAt}=    Set Variable    ${response.json()['isParked']['createdAt']}
    Set Test Variable    ${createdAt}

Delete Initial Entity
    Delete Entity    ${entity_id}
