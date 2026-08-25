*** Settings ***
Documentation       Check that purge entities deletes all entities in local mode

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Setup          Setup Initial Entities
Test Teardown       Delete Created Entities


*** Test Cases ***
060_03_02 Purge All Entities In Local Mode
    [Documentation]    Check that purge entities deletes all entities in local mode
    [Tags]    e-purge    5_6_21    6_4_3_3    since_v1.9.1

    ${response}=    Purge Entities
    ...    local=true

    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}

    ${response1}=    Query Entities
    ...    entity_types=Building
    ...    context=${ngsild_test_suite_context}
    Check SUT Not Containing Resources    ${response1.json()}


*** Keywords ***
Setup Initial Entities
    ${first_entity_id}=    Catenate    ${BUILDING_ID_PREFIX}060-04-1
    Set Test Variable    ${first_entity_id}
    ${create_response1}=    Create Entity Selecting Content Type
    ...    building-simple-attributes.jsonld
    ...    ${first_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response1.status_code}
    ${second_entity_id}=    Catenate    ${BUILDING_ID_PREFIX}060-04-2
    Set Test Variable    ${second_entity_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    building-simple-attributes.jsonld
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}

Delete Created Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
