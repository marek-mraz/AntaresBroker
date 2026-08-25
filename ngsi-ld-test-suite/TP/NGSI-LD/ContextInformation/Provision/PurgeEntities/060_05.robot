*** Settings ***
Documentation       Check that purge entities with keep parameter deletes all attributes except the specified ones from matching entities

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Entity    ${entity_id}


*** Test Cases ***
060_05_01 Purge Entities Keep Only Specific Attributes
    [Documentation]    Check that purge entities with keep parameter deletes all attributes except those specified while keeping the entities
    [Tags]    e-purge    5_6_21    6_4_3_3    since_v1.9.1

    # `type` added (was id+keep only): 5.6.21.4 and Table 6.4.3.3-1 both require
    # at least one of type/attrs/q/georel (or local), and keep/drop are
    # projection, not filters — see error.md 2026-08-09. The subject of this TP
    # (keep semantics) is unchanged.
    ${response}=    Purge Entities
    ...    id=${entity_id}
    ...    type=Building
    ...    keep=name
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}

    ${retrieve_response}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${retrieve_response.status_code}
    ${entity_json}=    Set Variable    ${retrieve_response.json()}
    Dictionary Should Contain Key    ${entity_json}    name
    Dictionary Should Not Contain Key    ${entity_json}    subCategory
    Dictionary Should Not Contain Key    ${entity_json}    airQualityLevel
    Dictionary Should Not Contain Key    ${entity_json}    almostFull


*** Keywords ***
Setup Initial Entity
    ${entity_id}=    Catenate    ${BUILDING_ID_PREFIX}060-06-1
    Set Test Variable    ${entity_id}
    ${create_response}=    Create Entity Selecting Content Type
    ...    building-simple-attributes.jsonld
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}
