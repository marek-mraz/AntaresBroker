*** Settings ***
Documentation       Check that the correct error type is returned when replacing an attribute with faulty data

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Initial Entity
Test Template       Replace Entity Attribute With Faulty Data


*** Variables ***
${entity_filename}      vehicle-speed-multi-instances.jsonld


*** Test Cases ***    ENTITY_ID    ATTR_ID    ATTRIBUTE_FILE_NAME    EXPECTED_STATUS_CODE
055_03_01 Replace Entity Attribute Giving An Invalid Entity ID
    [Tags]    ea-replace    5_6_19    6_7_3_3    since_v1.6.1
    invalidUri    isParked    vehicle-isParked-attribute.json    400
055_03_02 Replace Entity Attribute Giving A Nonexistent Entity ID
    [Tags]    ea-replace    5_6_19    6_7_3_3    since_v1.6.1
    urn:ngsi-ld:Vehicle:Nonexistent    isParked    vehicle-isParked-attribute.json    404
055_03_03 Replace Entity Attribute Giving An Invalid Attribute Name
    [Tags]    ea-replace    5_6_19    6_7_3_3    since_v1.6.1
    ${entity_id}    @invalid    vehicle-isParked-attribute.json    400
055_03_04 Replace Entity Attribute Giving A Nonexistent Attribute Name
    [Tags]    ea-replace    5_6_19    6_7_3_3    since_v1.6.1
    ${entity_id}    brandName    vehicle-isParked-attribute.json    404
055_03_05 Replace Entity Attribute Giving A Scope Attribute
    [Tags]    ea-replace    5_6_19    6_7_3_3    since_v1.6.1
    ${entity_id}    scope    vehicle-scope-attribute.json    400


*** Keywords ***
Replace Entity Attribute With Faulty Data
    [Documentation]    Check that the correct error type is returned when replacing an attribute with faulty data
    [Arguments]    ${target_entity_id}    ${attr_id}    ${attribute_replacement_filename}    ${expected_status_code}
    ${attribute}=    Load Test Sample
    ...    test_sample_file_path=entities/${attribute_replacement_filename}
    ...    test_sample_id=${entity_id}
    ${response}=    Replace Attribute Selecting Content Type
    ...    entity_id=${target_entity_id}
    ...    attr_id=${attr_id}
    ...    attribute_fragment=${attribute}
    ...    content_type=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}

Setup Initial Entity
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${entity_filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Initial Entity
    Delete Entity    ${entity_id}
