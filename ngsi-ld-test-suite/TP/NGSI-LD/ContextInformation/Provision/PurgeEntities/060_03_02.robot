*** Settings ***
Documentation       Check that purge entities deletes entities matching a q query

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Setup          Setup Initial Entities
Test Teardown       Delete Created Entities


*** Test Cases ***
060_03_02 Purge Entities Matching Q Query
    [Documentation]    Check that purge entities deletes entities matching a q query
    [Tags]    e-purge    5_6_21    6_4_3_3    since_v1.9.1

    ${response}=    Purge Entities
    ...    q=name=="Eiffel Tower"
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}

    ${retrieve_response}=    Retrieve Entity    id=${first_entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${retrieve_response.status_code}

    ${retrieve_response}=    Retrieve Entity    id=${second_entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${retrieve_response.status_code}


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
    ...    building-simple-attributes-second.jsonld
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}

Delete Created Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
