*** Settings ***
Documentation       Check the Attribute Projection Language or-operator spellings
...                 (CIM 009 clause 4.21): "either a comma or a pipe character can be
...                 used as alternative representations of the or operator". The
...                 official projection TPs use only the comma. A malformed projection
...                 (empty member) is rejected.
...
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Initial Entity
Suite Teardown      Delete Created Entity


*** Variables ***
${filename}=    building-simple-attributes.jsonld


*** Test Cases ***
421_01_01 Pipe And Comma Pick The Same Attributes
    [Documentation]    4.21: pick=name|airQualityLevel equals pick=name,airQualityLevel —
    ...    both keep exactly the picked attributes and drop the rest
    [Tags]    e-retrieve    4_21    since_v1.9.1
    ${comma}=    Retrieve Picked    name,airQualityLevel
    ${pipe}=    Retrieve Picked    name|airQualityLevel
    Check Response Status Code    200    ${comma.status_code}
    Check Response Status Code    200    ${pipe.status_code}
    Should Be Equal    ${comma.json()}    ${pipe.json()}
    ${keys}=    Evaluate    sorted($pipe.json().keys())
    ${has_sub}=    Evaluate    'subCategory' in $pipe.json()
    Should Not Be True    ${has_sub}    msg=unpicked attributes must be dropped
    ${has_name}=    Evaluate    'name' in $pipe.json()
    Should Be True    ${has_name}

421_01_02 An Empty Projection Member Is Rejected
    [Documentation]    4.21 grammar: ProjectionTerm admits no empty member — a doubled
    ...    or-operator is a violation
    [Tags]    e-retrieve    4_21    since_v1.9.1
    ${response}=    Retrieve Picked    name||airQualityLevel
    Check Response Status Code    400    ${response.status_code}


*** Keywords ***
Retrieve Picked
    [Arguments]    ${pick}
    ${context_link}=    Build Context Link    ${ngsild_test_suite_context}
    &{headers}=    Create Dictionary    Link=${context_link}
    &{params}=    Create Dictionary    pick=${pick}
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}
    ...    headers=${headers}
    ...    params=${params}
    ...    expected_status=any
    RETURN    ${response}

Create Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Created Entity
    Delete Entity    ${entity_id}
