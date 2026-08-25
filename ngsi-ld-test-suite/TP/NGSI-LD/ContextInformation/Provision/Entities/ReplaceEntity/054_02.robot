*** Settings ***
Documentation       Check that if the target entity ID is faulty an error is raised

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Initial Entity
Test Template       Replace an existing entity with a faulty ID


*** Variables ***
${entity_filename}                  building-simple-attributes.json
${entity_replacement_filename}      building-locatedAt-and-name.json


*** Test Cases ***    FAULTY_ENTITY_ID    EXPECTED_STATUS_CODE
054_02_01 Replace An Existing Entity Giving An Invalid Id
    [Tags]    e-replace    5_6_18    6_5_3_3    since_v1.6.1
    invalidUri    400
054_02_02 Replace An Existing Entity Without Giving An Id
    [Tags]    e-replace    5_6_18    6_5_3_3    since_v1.6.1
    ${EMPTY}    400
054_02_03 Replace An Existing Entity Giving A Nonexistent Id
    [Tags]    e-replace    5_6_18    6_5_3_3    since_v1.6.1
    urn:ngsi-ld:Building:Nonexistent    404


*** Keywords ***
Replace an existing entity with a faulty ID
    [Documentation]    Check that if the target entity ID is faulty an error is raised
    [Arguments]    ${faulty_entity_id}    ${expected_status_code}
    ${entity}=    Load Entity
    ...    entity_file_name=${entity_replacement_filename}
    ...    entity_id=${faulty_entity_id}
    ${response}=    Replace Entity Selecting Content Type
    ...    entity_id=${faulty_entity_id}
    ...    entity_fragment=${entity}
    ...    content_type=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}

Setup Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${entity_filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}
    Check Response Status Code    201    ${response.status_code}

Delete Initial Entity
    Delete Entity    ${entity_id}
