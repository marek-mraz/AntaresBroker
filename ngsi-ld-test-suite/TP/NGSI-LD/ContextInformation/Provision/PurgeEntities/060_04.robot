*** Settings ***
Documentation       Check that purge entities with drop parameter deletes only the specified attributes from matching entities

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Entity    ${entity_id}


*** Test Cases ***
060_04_01 Purge Entities Drop Specific Attributes
    [Documentation]    Check that purge entities with drop parameter removes only the specified attributes while keeping the entities
    [Tags]    e-purge    5_6_21    6_4_3_3    since_v1.9.1

    ${response}=    Purge Entities
    ...    type=Building
    ...    drop=name
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}

    ${retrieve_response}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${retrieve_response.status_code}
    ${entity_json}=    Set Variable    ${retrieve_response.json()}
    Dictionary Should Not Contain Key    ${entity_json}    name
    Dictionary Should Contain Key    ${entity_json}    subCategory


*** Keywords ***
Setup Initial Entity
    ${entity_id}=    Catenate    ${BUILDING_ID_PREFIX}060-05-1
    Set Test Variable    ${entity_id}
    ${create_response}=    Create Entity Selecting Content Type
    ...    building-simple-attributes.jsonld
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}
