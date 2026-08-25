*** Settings ***
Documentation       Check that, given an inclusive registration with the noOverwrite flag, appending entity attributes creates the new attributes in the Context Source accordingly but does not modify existing ones.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Create Entity And Registration On The Context Broker And Start Context Source Mock Server
Test Teardown       Delete Registration And Stop Context Source Mock Server

*** Variables ***
${entity_payload_filename}              vehicle-simple-attributes.jsonld
${fragment_filename}                    vehicle-speed-isParked-fragment.json
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-redirection-ops.jsonld

*** Test Cases ***
D003_02_inc Append Entity Attribute
    [Documentation]    Check that an entity attribute is appended and the inclusive registration forwards the request to the Context Source with the noOverwrite flag
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    additive-inclusive    4_3_6_2    5_6_3

    # context= required: a context-less retrieve compacts against the CORE context (5.5.7/6.3.5)
    # and test-suite attrs come back as full IRIs, never as 'speed'/'isParked'.
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    ${old_body}=    Get From Dictionary    ${response.json()}    isParked

    Set Stub Reply    POST    /ngsi-ld/v1/entities/${entity_id}/attrs/    207
    # ETSI tool bug fixed: without a JSON-LD context the fragment expands against the default
    # @context (6.3.5) and can never match the registration's compound-context propertyNames.
    ${response}=    Append Entity Attributes With Parameters
    ...    ${entity_id}
    ...    ${fragment_filename}
    ...    ${CONTENT_TYPE_JSON}
    ...    noOverwrite
    ...    context=${ngsild_test_suite_context}
    Wait For Request
    Check Response Status Code    207    ${response.status_code}

    ${stub}=    Get Request Url Params    options
    Should Contain    ${stub}    noOverwrite

    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    ${new_body}=    Get From Dictionary    ${response.json()}    isParked
    Should Have Value In Json    ${response.json()}    $.speed
    Should Be Equal    ${old_body}    ${new_body}

*** Keywords ***
Create Entity And Registration On The Context Broker And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

    ${response}=    Create Entity    ${entity_payload_filename}    ${entity_id}
    Check Response Status Code    201    ${response.status_code}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    mode=inclusive
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}
    Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Delete Entity    ${entity_id}
    Stop Context Source Mock Server
